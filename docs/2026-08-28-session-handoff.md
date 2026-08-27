# セッション引き継ぎメモ（2026-08-28）

このドキュメントは、tailcat を cgo で C ABI 化して Rust から使う `p2p-lib` プロジェクトの
初回セッションで何をやったか、何が決まったか、何で詰まったか、次に何をやるべきかをまとめた
引き継ぎメモです。実装の詳細は各ソースのコメントと [README.md](../README.md) を参照してください。

## プロジェクトの目的

[tailcat](https://github.com/tailscale/tailcat)（Tailscale のデータプレーンだけを使い、
コントロールプレーン無しで WireGuard + DERP 経由の P2P 接続ができる Go ライブラリ）を
cgo で C ABI 共有ライブラリ (DLL/so/dylib) にビルドし、Rust をはじめ複数言語・
マルチプラットフォームから使えるようにする。

## 決まった設計方針（経緯つき）

1. **API 粒度は高レベル**：`Server`/`Client` の `Start`/`ConnBlob`/`DialTCPPort`/`Ping` などを
   ハンドルベースの C 関数にそのままエクスポート。ストリーミング I/O はハンドル越しに
   Read/Write を繰り返す形。
2. **ブロッキング呼び出し、コールバックなし**：cgo 呼び出しは専用 OS スレッドで実行される
   ため、Rust 側をブロックしても Go ランタイム全体は止まらない。`Server.OnTCP` のような
   コールバックベースの API は、内部で `chan net.Conn` にキューして
   `tailcat_server_accept(handle, timeout_ms)` というブロッキング関数に変換している
   （[tailcat-cgo/wrapper.go](../tailcat-cgo/wrapper.go) 参照）。
3. **永続化は JSON 文字列の受け渡しのみ、ファイル I/O は一切しない**：`PrivateKey`
   （鍵ペア）や `Server` の allowed-clients リストは `tailcat_privatekey_generate()` /
   `tailcat_server_state()` などが JSON 文字列を返すだけ。どこに保存するか
   （ファイル、OS キーチェーン、Android Keystore、ブラウザの IndexedDB 等）は
   呼び出し側言語・プラットフォームの責務。理由：Android/iOS はサンドボックスやセキュア
   ストレージ API が Go のファイル I/O と根本的に相容れないため。
4. **中核は言語非依存の C ABI、Rust はその上の最初のバインディング**：`tailcat_cgo.h` を
   正としており、将来 Python/C#/Swift/Kotlin 向けバインディングを同じ共有ライブラリの上に
   追加できる設計。Rust クレートは `build.rs` が Go 側を自動ビルドしてリンクする
   cdylib 一体型（`cargo add` するだけで完結）。

## ディレクトリ構成

```
p2p-lib/
  tailcat/        tailcat 本体 (git submodule, github.com/tailscale/tailcat)
  tailcat-cgo/     tailcat を cgo で C ABI エクスポートする Go モジュール
  rust/
    p2p-lib-sys/   低レベル FFI bindings (build.rs が tailcat-cgo を自動ビルド)
    p2p-lib/       安全な高レベル Rust API (Server / Client / Conn / ConnReader / ConnWriter)
      examples/    server.rs, client.rs, chat.rs (CLI チャット)
  flutter/
    p2p_lib_flutter/  Flutter FFI プラグイン (Server/Client/Conn を Isolate.run 経由で公開)
      example/         2人チャットの Flutter サンプルアプリ
  docs/            このファイルのような引き継ぎ・調査メモ
```

## 実装済みのもの

- `tailcat-cgo/`：`handle.go`（ハンドル管理・エラー保持）、`keys.go`（PrivateKey の JSON
  シリアライズ）、`wrapper.go`（`//export` された23個のC ABI関数）
- `rust/p2p-lib-sys/`：`build.rs` が `go build -buildmode=c-shared` を自動実行し、
  **Windows 特有の問題**（後述）を自動解決してリンクする
- `rust/p2p-lib/`：`Server`/`Client`/`Conn`/`ConnReader`/`ConnWriter` の安全な Rust API。
  `Conn::split()` で読み取り/書き込みを別スレッドに分離できる（チャット example で使用）
- 動作するサンプル3本：`server.rs`/`client.rs`（単純な送受信）、`chat.rs`（双方向 CLI チャット）
  → いずれも実際に2プロセス間で DERP リレー経由 → direct UDP パスへのアップグレードまで
  含めてエンドツーエンドで動作確認済み

## Windows ビルドで詰まった点と解決策

Rust のデフォルトターゲットは `x86_64-pc-windows-msvc`（`link.exe` でリンク）だが、
Go の cgo `-buildmode=c-shared` は Windows では mingw-w64 gcc を使い、`.dll` だけを生成して
MSVC 互換の `.lib` インポートライブラリを作らない。そのままだと
`LINK : fatal error LNK1181: 入力ファイル 'tailcat_cgo.lib' を開けません。` で失敗する。

**解決策**（[rust/p2p-lib-sys/build.rs](../rust/p2p-lib-sys/build.rs) の
`generate_msvc_import_lib` 関数）：MSVC Build Tools 同梱の `dumpbin.exe /exports` で
DLL のエクスポートシンボル一覧を取得 → `.def` ファイルに変換 → `lib.exe /def` で
MSVC 互換の `.lib` を自動生成。`find_msvc_bin_dir()` が Program Files 配下の
Build Tools インストールを自動検索するので、手作業は不要（rustc 自体がビルドできる環境なら
動くはず）。

また、生成した `.dll` は `OUT_DIR` に置かれるだけでは実行時に見つからないため、
`build.rs` が `target/<profile>/`、`target/<profile>/examples/`、`target/<profile>/deps/`
の3箇所にコピーしている。

## gVisor netstack 由来の落とし穴（Close 順序）

tailcat の TCP スタックは OS ではなくこのプロセス内（gVisor netstack）で完結しているため、
`Conn::close()` 直後にプロセスが終了すると、送信済みデータの FIN が相手に届く前に失われる
ことがある。

実際にこれで chat example の初期実装がハマった：クライアントが `write` 後すぐ `close()`
（完全クローズ）していたら、サーバー側の `read_to_string` がブロックしたまま応答が返らな
かった。**半クローズ**（`Conn::close_write()` / Go 側の新規 `tailcat_conn_close_write`、
`net.Conn` の `CloseWrite()` を叩くだけ）を先に呼び、読み取り側の EOF 検知を可能にしてから
完全クローズする流れに直して解決。tailcat CLI 本家も同じパターン（`DrainTCP` の利用）を
踏襲している。

`tailcat_conn_read` の戻り値も、当初 EOF とエラーを区別できない設計だったのを、
`-2` を EOF 専用センチネル値とする規約に直した（`errors.Is(err, io.EOF)` で判定）。

## ライセンス確認（結論：問題なし）

`go list -deps -f '{{.Module}}' .` で実際にビルド・リンクされる依存だけに絞り込んだところ
35モジュール。すべて BSD-3-Clause / MIT / Apache-2.0 のいずれかで、GPL/AGPL 等の
コピーレフトは一切なし。「`golang.zx2c4.com/wireguard/windows`」という名前に一瞬身構えたが、
中身は MIT ライセンスのユーティリティコードで、WireGuard 本体（Linux カーネルモジュール、
GPLv2）とは無関係。

商用・非公開・公開いずれも問題なく配布・組み込み可能。ただし Apache-2.0/BSD/MIT は
著作権表示の同梱が条件なので、配布時に THIRD_PARTY_LICENSES 的なファイルを用意するのが
望ましい（**未着手**）。

`go-licenses` ツールは Windows + 最近の Go バージョンとの相性が悪く（標準ライブラリ
パッケージを「モジュール情報なし」としてエラー扱いにする）使えなかったため、
`go list -m all` の全582モジュール中、実際にリンクされる35個だけを手動確認する方式を取った。

## Android 検証（済み・成功）

環境：Android Studio 経由で NDK 30.0.16138531 をユーザーディレクトリ
（`%LOCALAPPDATA%\Android\Sdk\ndk\30.0.16138531`）に導入。
※ 最初は `Program Files (x86)\Android\android-sdk` に `sdkmanager` で NDK を追加しようと
したが書き込み権限がなく失敗、単体 NDK zip の直接ダウンロードもネットワークが遅すぎて
断念。**Android Studio の GUI からインストールするのが一番安定していた。**

### ビルド結果

3 ABI すべてでビルド成功（cgo が gVisor/wireguard-go 由来のコードで問題を起こすのではという
事前の懸念は杞憂だった）：

```sh
NDK=".../Sdk/ndk/30.0.16138531"
BIN="$NDK/toolchains/llvm/prebuilt/windows-x86_64/bin"

# arm64-v8a
GOOS=android GOARCH=arm64 CGO_ENABLED=1 \
  CC="$BIN/aarch64-linux-android21-clang.cmd" \
  go build -buildmode=c-shared -o tailcat_cgo_arm64.so .

# armeabi-v7a
GOOS=android GOARCH=arm GOARM=7 CGO_ENABLED=1 \
  CC="$BIN/armv7a-linux-androideabi21-clang.cmd" \
  go build -buildmode=c-shared -o tailcat_cgo_armv7.so .

# x86_64 (emulator)
GOOS=android GOARCH=amd64 CGO_ENABLED=1 \
  CC="$BIN/x86_64-linux-android21-clang.cmd" \
  go build -buildmode=c-shared -o tailcat_cgo_x86_64.so .
```

### 実機検証結果

実機：Fire TV Stick（armeabi-v7a, API 28）、Wi-Fi 経由の ADB (`adb connect`) で接続。
JNI を書く代わりに、`dlopen`/`dlsym` で `.so` をロードする最小限の C プログラムを
NDK でコンパイルし、デバイス上で直接実行して検証した（[Kotlin/JNI バインディングはまだ
書いていない — 次のステップ](#次にやるべきこと) 参照）。

1. `tailcat_privatekey_generate()` の呼び出し → 成功（鍵ペアの JSON が返る）
2. `Server::new → start → ConnBlob` の一連の流れ → 成功（実際に DERP region 304 に接続し、
   有効なトークンを発行）

ログ上、`magicsock: [warning] failed to force-set UDP read/write buffer size: operation not
permitted` という警告が出たが、これはこの Fire TV 環境の権限制約によるもので、Go 側のコメント
にある通り「スループットのみに影響」なので機能的には無視できる。

### adb 特有の詰まりポイント

- **`adb push` が `remote fchown failed: Operation not permitted` で失敗**：この
  Fire TV のような root なし・制限された `shell` ユーザー権限の `adbd` では、push 内部の
  chown 処理が権限不足で丸ごと失敗する（部分的に転送されたように見えて実際は空ファイルに
  なる）。**回避策**：`base64 -w0 <local file> | adb shell "base64 -d > <remote path>"` で
  シェル経由の base64 転送に切り替えると成功する（生バイナリを素の `cat` パイプで送ると
  改行コード変換等で壊れるので、`base64` を挟むのが必須）。
- **Git Bash の自動パス変換**：`adb shell mkdir -p /data/local/tmp/...` のような POSIX
  パスを引数に渡すと、Git Bash が `C:/Program Files/Git/data/local/tmp/...` のような
  Windows パスに勝手に変換してしまう。**回避策**：該当引数の先頭に `//` を付ける
  （`//data/local/tmp/...`）と変換されない。`MSYS_NO_PATHCONV=1` は push のローカル側
  パスまで無変換にしてしまい壊れるので、`push` には使わないこと。

### 次にやるべきこと（Android）

- 今回は C の `dlopen`/`dlsym` で直接検証しただけで、実際の Kotlin/Java 向け **JNI
  バインディングはまだ書いていない**。次はこの `.so` を JNI 経由で Kotlin から呼ぶ
  最小サンプル（Android Studio プロジェクト）を作るのが自然な次のステップ。
- `Conn` の Read/Write ループを Kotlin 側の Coroutine とどう協調させるか
  （Rust の `spawn_blocking` 相当）は未検討。

## macOS 検証（済み・成功、2026-08-28 追記）

環境：Apple Silicon Mac（arm64）、Xcode 同梱コマンドラインツール、Go 1.26.1。

### ビルド結果

```sh
cd tailcat-cgo
go build -buildmode=c-shared -o /tmp/tailcat_cgo_macos_arm64.dylib .
```

追加のフラグ・環境変数一切不要でビルド成功（`otool -L` で
CoreFoundation/Security/libresolv/libSystem のみリンクされていることを確認）。
Windows のような MSVC インポートライブラリ問題は macOS には存在しない
（`build.rs` の macOS 分岐は `dylib` 指定のみでそのまま動作）。

### Rust クレート経由の e2e 検証

`cargo build --examples` が cgo ビルドを自動実行してリンクまで成功。
`server`/`client` example を同一マシン上の2プロセスとして実行し、
DERP リレー（region 303/1）経由でハンドシェイク → メッセージ送受信 → 半クローズによる
EOF 検知まで、Windows で確認済みだった挙動が macOS でも同一に動作することを確認。

### 次にやるべきこと（macOS）

- 今回はコマンドライン実行のみ検証。macOS デスクトップアプリ（.app バンドル）への
  組み込み・コード署名・サンドボックス環境での動作は未検証。
- Network Extension や entitlements が必要になるかは未調査（tailcat 自体は
  ユーザー空間 netstack で完結するため通常のアプリ権限で動く可能性が高いが未確認）。

## iOS 検証（済み・成功、2026-08-28 追記）

環境：上記と同じ Apple Silicon Mac。iOS は動的リンクが原則不可なため
`c-shared` ではなく `c-archive` を使用。

### ビルド結果

実機向け（arm64）とシミュレータ向け（arm64、Apple Silicon Mac 上で動くシミュレータ）の
両方をビルドし、`xcodebuild -create-xcframework` で1つの `.xcframework` にまとめる
ところまで成功：

```sh
# 実機 (iphoneos)
SDK=$(xcrun --sdk iphoneos --show-sdk-path)
GOOS=ios GOARCH=arm64 CGO_ENABLED=1 \
  SDKROOT="$SDK" CC=$(xcrun --sdk iphoneos -f clang) \
  CGO_CFLAGS="-isysroot $SDK -miphoneos-version-min=13.0 -arch arm64" \
  CGO_LDFLAGS="-isysroot $SDK -miphoneos-version-min=13.0 -arch arm64" \
  go build -buildmode=c-archive -o tailcat_cgo_ios_arm64.a .

# シミュレータ (iphonesimulator)
SDK=$(xcrun --sdk iphonesimulator --show-sdk-path)
GOOS=ios GOARCH=arm64 CGO_ENABLED=1 \
  SDKROOT="$SDK" CC=$(xcrun --sdk iphonesimulator -f clang) \
  CGO_CFLAGS="-isysroot $SDK -mios-simulator-version-min=13.0 -arch arm64" \
  CGO_LDFLAGS="-isysroot $SDK -mios-simulator-version-min=13.0 -arch arm64" \
  go build -buildmode=c-archive -o tailcat_cgo_iossim_arm64.a .

xcodebuild -create-xcframework \
  -library tailcat_cgo_ios_arm64.a -headers <device-headers-dir> \
  -library tailcat_cgo_iossim_arm64.a -headers <sim-headers-dir> \
  -output tailcat_cgo.xcframework
```

いずれも追加パッチ不要でビルド成功（Android の NDK ほどの罠はなかった）。

### 実機能検証（シミュレータ内実行、成功）

JNI 検証と同様、Swift バインディングを書く前に C の最小プログラムで直接シンボルを
叩いて検証した。`clang` でシミュレータ向けバイナリを静的リンクし、
**`xcrun simctl spawn <UDID> <バイナリ>`** で起動中のシミュレータ内で実行
（ホスト macOS 上で直接実行すると `dyld[...]: DYLD_ROOT_PATH not set for simulator
program` で失敗するため、必ず `simctl spawn` 経由にすること）。

1. `tailcat_privatekey_generate()` → 成功（鍵ペア JSON が返る）
2. `tailcat_server_new` → `tailcat_server_start` → `tailcat_server_connblob` の一連の
   流れ → 成功。実際に DERP region 304 に接続し、有効な conn blob トークンを発行
   （ログは Android 実機検証時と同一パターン：gVisor netstack 初期化 → WireGuard
   デバイス起動 → DERP ホームリレー確立）

iOS シミュレータのサンドボックスから外部の DERP サーバーへの UDP/HTTPS アクセスが
問題なく通ることが実証された。実機（実際の iPhone/iPad 実体）での検証はまだ行って
いないが、シミュレータはネットワークスタック自体はホスト共有のため、ネットワーク到達性
に関する不確実性はほぼ解消されたと考えてよい。

### 次にやるべきこと（iOS）

- **Swift バインディングはまだ書いていない**。次はこの `.xcframework` を Swift Package
  として組み込み、Swift から最小サンプル（Android の Kotlin 相当）を作るのが自然な
  次のステップ。
- **実機（シミュレータでない実体の iPhone/iPad）での検証は未実施**。実機は開発者
  署名・provisioning profile が必要なため、実機が用意でき次第別途検証すること。
- App Store 配布を見据えるなら、バックグラウンドでの WireGuard/UDP 処理が
  Network Extension（`NEPacketTunnelProvider`）内での実行を要求される可能性が高く、
  これは未検証・未調査（フォアグラウンドでの直接呼び出しは今回確認済み）。
- `c-archive` が生成する `.a` はサイズが大きい（arm64 単体で約46MB）。リリースビルド時の
  シンボルストリップ・アーカイブサイズ最適化は未検討。

## その他の未着手事項

- Apache-2.0/BSD/MIT 依存の著作権表示同梱（THIRD_PARTY_LICENSES ファイル）
- Linux での cgo ビルド・実機検証（Windows でのビルドしか試していない。おそらく普通に
  動くはずだが未確認）
- 鍵の真正性検証・なりすまし対策など、アプリ層で追加すべきセキュリティレイヤーの議論は
  会話ログ上で行ったが（グローバル IP 露出のトレードオフ、通信路暗号化は tailcat が担保する
  一方で相手認証や保存データの暗号化はアプリ側の責務、という結論）、ライブラリの実装には
  まだ反映していない
