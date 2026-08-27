# p2p_lib_flutter

Flutter FFI bindings for [tailcat-cgo](../../tailcat-cgo) -- WireGuard + DERP
relayed P2P networking with no control plane or account required. Mirrors
the design of [`rust/p2p-lib`](../../rust/p2p-lib): `Server`/`Client`/`Conn`
wrap the same 23-function C ABI, one level up from raw FFI.

See [example/lib/main.dart](example/lib/main.dart) for a working two-person
chat app (host or join by pasting a connection token) that exercises the
whole API end to end.

## API

```dart
import 'package:p2p_lib_flutter/p2p_lib_flutter.dart';

// Host side
final server = await Server.create();
await server.start();
final token = await server.connBlob(); // share this with the peer
final conn = await server.accept(const Duration(seconds: 120));

// Peer side
final client = await Client.create(token);
await client.ping(const Duration(seconds: 30));
final conn = await client.dialTcpPort(0, const Duration(seconds: 30));

// Either side, once connected
await conn!.writeAll(utf8.encode('hello\n'));
final reply = await conn.read(4096);
await conn.close();
```

Every `Server`/`Client`/`Conn` method does blocking native I/O and always
dispatches the actual `tailcat_*` call onto a background isolate via
`Isolate.run` -- safe to call from a widget's build/event handlers without
freezing the UI.

Persistence (identity keys, allow-lists) is JSON-string in/out only --
`PrivateKey.generate()`/`.json` -- with no file I/O of its own. Where you
store that JSON (Keychain, Android Keystore, secure storage, ...) is your
app's responsibility, same as the Rust crate.

## How the native library gets built

There's no vendored copy of tailcat's Go source here -- every platform's
build shells out to `go build` against the sibling
[`../../tailcat-cgo`](../../tailcat-cgo) Go module, the same way
`rust/p2p-lib-sys/build.rs` does for Rust:

* **macOS**: [`macos/p2p_lib_flutter.podspec`](macos/p2p_lib_flutter.podspec)'s
  `prepare_command` runs `go build -buildmode=c-shared`, producing
  `libtailcat_cgo.dylib`, which CocoaPods bundles into the app's
  `Contents/Frameworks` via `vendored_libraries`. Go stamps the dylib's
  install name as a bare filename instead of `@rpath/...`, which breaks
  `dlopen` from inside the app bundle at launch -- the prepare_command
  patches it with `install_name_tool -id @rpath/...` to fix that (see the
  comment in the podspec and in
  [`lib/src/dylib.dart`](lib/src/dylib.dart) for the failure mode this
  works around).
* **iOS**: [`ios/p2p_lib_flutter.podspec`](ios/p2p_lib_flutter.podspec)
  builds a static archive instead (`-buildmode=c-archive`) since iOS
  forbids third-party dynamic libraries, and links it straight into the
  app binary. **The prepare_command currently ships the device (arm64,
  `iphoneos` SDK) slice only** -- if you need to run on the iOS
  Simulator, edit the podspec's last line to copy
  `libtailcat_cgo_sim.a` instead of `libtailcat_cgo_device.a` before
  `pod install`, then switch it back for device/App Store builds. A
  proper per-SDK slice selection (so one `pod install` works for both) is
  the natural next step here.  Symbols are looked up via
  `DynamicLibrary.process()` in `lib/src/dylib.dart` rather than opening a
  named library, since they're statically linked into the app.
* **Android**: [`src/CMakeLists.txt`](src/CMakeLists.txt) invokes `go
  build -buildmode=c-shared` per ABI (mapping Gradle's `ANDROID_ABI` to
  the matching `GOOS=android GOARCH=...`), producing `libtailcat_cgo.so`
  bundled the normal Gradle/CMake way.

This means **Go 1.21+ and a working cgo toolchain must be installed and on
`PATH`** wherever you build this plugin (in addition to the usual
Xcode/Android NDK toolchains) -- there is currently no prebuilt-binary
fallback.

## Regenerating bindings

`lib/src/tailcat_cgo_bindings_generated.dart` is generated from
[`src/tailcat_cgo.h`](src/tailcat_cgo.h) (a hand-written, cleaned-up
mirror of the header Go's cgo build emits -- see the comment at the top of
that file) by `package:ffigen`:

```sh
dart run ffigen --config ffigen.yaml
```

Regenerate it if `tailcat-cgo`'s exported C ABI (`tailcat-cgo/wrapper.go`)
changes, keeping `src/tailcat_cgo.h` in sync by hand first.

## Platforms tried so far

* **macOS** (Apple Silicon): built, launched, and end-to-end verified --
  `Server`/`Client`/`Conn` exchanged a message over a real DERP relay from
  a standalone Dart script driving the plugin's own `lib/` code (see
  `TAILCAT_CGO_DYLIB_PATH` in `lib/src/dylib.dart`, an env var override
  useful for exactly this kind of out-of-bundle testing) and via the built
  `.app`.
* **iOS Simulator** (arm64): builds and launches cleanly (no dyld/link
  errors); the full `Server`/`Client` chat flow has not been driven
  end-to-end through the Simulator UI yet.
* **iOS device**: untested through Flutter (the underlying tailcat-cgo
  static archive was verified standalone; see
  `docs/2026-08-28-session-handoff.md` in the repo root).
* **Android**: untested through Flutter (previously verified working via
  raw `dlopen`/`dlsym` against the same tailcat-cgo `.so`; see the same
  handoff doc).
* **Linux/Windows**: untested; `src/CMakeLists.txt` should build them the
  same way `rust/p2p-lib-sys/build.rs` does but this hasn't been tried.
