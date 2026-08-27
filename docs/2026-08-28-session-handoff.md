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

## iOS（未着手）

Xcode（＝ macOS）が必須で、このマシンは Windows のためビルドはおろか検証の着手すらして
いない。tailscale.com 本体に公式 iOS アプリの実績があるため、依存パッケージレベルの
互換性は期待できる。見込みとしては：

- `GOOS=ios GOARCH=arm64 CGO_ENABLED=1 go build -buildmode=c-archive -o tailcat_cgo.a .`
  で静的ライブラリ + ヘッダを生成し、`.xcframework` として Xcode に組み込む
  （iOS は動的リンクが原則不可なので `c-shared` ではなく `c-archive`）
- ビルドが通ったら、Swift から最小限のリンクテストをする、という Android と同じ流れを踏襲
- App Store 配布を見据えるなら、バックグラウンドでの WireGuard/UDP 処理が
  Network Extension（`NEPacketTunnelProvider`）内での実行を要求される可能性が高く、
  これは未検証・未調査

**macOS 環境が用意でき次第、この節を更新して検証ログを残すこと。**

## その他の未着手事項

- Apache-2.0/BSD/MIT 依存の著作権表示同梱（THIRD_PARTY_LICENSES ファイル）
- Linux での cgo ビルド・実機検証（Windows でのビルドしか試していない。おそらく普通に
  動くはずだが未確認）
- macOS デスクトップでの cgo ビルド・実機検証（同上）
- 鍵の真正性検証・なりすまし対策など、アプリ層で追加すべきセキュリティレイヤーの議論は
  会話ログ上で行ったが（グローバル IP 露出のトレードオフ、通信路暗号化は tailcat が担保する
  一方で相手認証や保存データの暗号化はアプリ側の責務、という結論）、ライブラリの実装には
  まだ反映していない
