# p2p-lib

[tailcat](https://github.com/tailscale/tailcat)（Tailscale のデータプレーンだけを使い、
コントロールプレーン無しで P2P 接続できる Go ライブラリ）を C ABI 共有ライブラリ
(DLL/so/dylib) としてビルドし、Rust から（将来的には他言語からも）安全に使えるようにする
プロジェクトです。

## 構成

```
p2p-lib/
  tailcat/        tailcat 本体 (git submodule, github.com/tailscale/tailcat)
  tailcat-cgo/     tailcat を cgo で C ABI エクスポートする Go モジュール
  rust/
    p2p-lib-sys/   低レベル FFI bindings (build.rs が tailcat-cgo を自動ビルド)
    p2p-lib/       安全な高レベル Rust API (Server / Client / Conn)
```

言語非依存の核は `tailcat-cgo/` が生成する C ABI 共有ライブラリです。`rust/` はその上に
乗る最初のバインディングで、同じ C ABI (`tailcat_cgo.h`) の上に将来 Python / C# / Swift /
Kotlin 向けのバインディングを追加できる設計にしています。

## 使い方 (Rust)

```rust
use std::io::{Read, Write};
use std::time::Duration;

// サーバー側
let mut server = p2p_lib::Server::new()?;
server.start()?;
println!("token: {}", server.conn_blob()?);
let mut conn = server.accept(Duration::from_secs(60))?.unwrap();
conn.write_all(b"hello\n")?;

// クライアント側 (別プロセス、上のトークンを渡す)
let client = p2p_lib::Client::new("tc...")?;
let mut conn = client.dial_tcp_port(0, Duration::from_secs(10))?;
let mut buf = String::new();
conn.read_to_string(&mut buf)?;
```

動作するサンプルは [rust/p2p-lib/examples/server.rs](rust/p2p-lib/examples/server.rs) /
[rust/p2p-lib/examples/client.rs](rust/p2p-lib/examples/client.rs) を参照してください。

```sh
cd rust
cargo run --example server
# 別ターミナルで、上で表示されたトークンを渡す
cargo run --example client -- <token>
```

対話的な CLI チャットのサンプルは
[rust/p2p-lib/examples/chat.rs](rust/p2p-lib/examples/chat.rs) にあります。同じバイナリが
引数なしなら「ホスト（待受）」、トークンを引数に渡すと「参加者」として動きます。

```sh
cd rust
cargo run --example chat
# 別ターミナル（または別マシン）で、表示されたトークンを渡す
cargo run --example chat -- <token>
```

行を入力して Enter で送信、相手からのメッセージは別スレッドで随時 `peer> ...` として
表示されます。Ctrl-D で自分のセッションを終了すると、`Conn::split()` で分離した書き込み側だけ
`close_write()`（半クローズ）してから受信スレッドの終了を待ち、最後に `Server`/`Client` を
`close()` して片付けます（内部で `DrainTCP` が走るので、送信済みデータの取りこぼしを防ぎます）。

## 鍵・接続情報の永続化について

このライブラリの Go/cgo 層は一切ファイル I/O を行いません。`PrivateKey::to_json()` /
`Server::state_json()` は素の JSON 文字列を返すだけで、それをどこに（ファイル、OS の
キーチェーン、暗号化された SharedPreferences、ブラウザの IndexedDB 等）どう保存するかは
呼び出し側言語・プラットフォームの責務です。デスクトップ・モバイル・ブラウザで保存先の
流儀が大きく異なるため、意図的にこの層を FFI 境界の外に出しています。

## ビルド要件

- Go 1.26.5 以上
- Rust (stable)
- **Windows**: mingw-w64 gcc (`x86_64-w64-mingw32-gcc`) — cgo の C コンパイラとして使用。
  Rust のデフォルトターゲットは `x86_64-pc-windows-msvc` なので、`build.rs` は Go が生成した
  `.dll` から `dumpbin.exe`/`lib.exe`（MSVC Build Tools 同梱）を使って MSVC 互換の
  `.lib` インポートライブラリを自動生成します。VS Build Tools（あるいは Visual Studio）が
  入っていて `rustc` 自体がこの環境でビルドできるなら、追加の手作業は不要です。
- **Linux/macOS**: 通常の cgo ビルド (`gcc`/`clang`) で動作する想定です。

## モバイル対応について（Android / iOS）

C ABI の設計自体はモバイルでも通用する形にしていますが、このリポジトリでの実ビルド検証は
デスクトップ（主に Windows）に限定しており、Android Studio / Xcode を用いた実機検証は
行っていません。将来対応する場合の見通しは以下の通りです。

- **Android**: `GOOS=android GOARCH=arm64 CGO_ENABLED=1` + Android NDK の clang
  (`aarch64-linux-android21-clang` 等) を `CC` に指定すれば `-buildmode=c-shared` で `.so`
  を生成できる見込みです。Kotlin/Java からは JNI 経由で `tailcat_cgo.h` の関数を直接叩くか、
  Go 公式の `gomobile bind`（独自のオブジェクト変換規約を持つ）を使う2つの道があります。
- **iOS**: `GOOS=ios GOARCH=arm64` で `-buildmode=c-archive`（iOS は動的リンク不可のため
  静的 `.a` + ヘッダ）を生成し、`.xcframework` として Xcode に組み込む形になる見込みです。
  ビルドには Xcode（＝macOS）が必須です。
- **モバイル特有の制約**: tailcat は WireGuard (userspace) + UDP ホールパンチングに依存する
  ため、iOS のバックグラウンド実行制限（Network Extension 内での実行が必要になる可能性）や
  Android のバッテリー最適化 (Doze) による UDP 接続の切断など、デスクトップにはない制約が
  あります。これらは未検証で、今後の課題です。

## Close 順序について（重要）

tailcat の TCP スタックはプロセス内 (gVisor netstack) で完結しているため、書き込んだデータが
実際に相手に届く前にプロセスが終了すると、送信済みの FIN が失われることがあります。

- 送信を終えて相手からの応答も読みたい場合は、完全な `close()` の前に半クローズ
  （Rust: `Conn::close_write()`）を呼んでください。
- プロセスをすぐ終了する direct な用途（サーバーを畳んで即終了、など）では、
  `Server::close()` を呼ぶことで内部的に `DrainTCP` を実行してから閉じるため、
  作りっぱなしで `drop` するより安全です。
