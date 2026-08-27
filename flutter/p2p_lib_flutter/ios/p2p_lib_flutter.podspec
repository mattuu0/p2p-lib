#
# To learn more about a Podspec see http://guides.cocoapods.org/syntax/podspec.html.
# Run `pod lib lint p2p_lib_flutter.podspec` to validate before publishing.
#
Pod::Spec.new do |s|
  s.name             = 'p2p_lib_flutter'
  s.version          = '0.0.1'
  s.summary          = 'Flutter FFI bindings for tailcat-cgo (P2P networking).'
  s.description      = <<-DESC
Flutter FFI bindings for tailcat-cgo, a cgo C ABI wrapper around tailcat
(Tailscale's data-plane-only P2P library).
                       DESC
  s.homepage         = 'http://example.com'
  s.license          = { :file => '../LICENSE' }
  s.author           = { 'Your Company' => 'email@example.com' }

  # No Objective-C/Swift sources of our own; the actual implementation is
  # the Go cgo static archive built by the script phase below. iOS forbids
  # third-party dynamic linking, so we build with -buildmode=c-archive
  # (unlike the macOS podspec, which uses a dylib) as both a device
  # (arm64) and simulator (arm64/x86_64) slice, fat-combined for the sim.
  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'

  s.vendored_libraries = 'libtailcat_cgo.a'

  s.prepare_command = <<-CMD
    set -e
    cd "#{File.dirname(__FILE__)}/../../../tailcat-cgo"
    OUT="#{File.dirname(__FILE__)}"

    build_slice() {
      sdk="$1"; min_flag="$2"; arch="$3"; out="$4"
      sdkroot=$(xcrun --sdk "$sdk" --show-sdk-path)
      cc=$(xcrun --sdk "$sdk" -f clang)
      GOOS=ios GOARCH=arm64 CGO_ENABLED=1 \\
        SDKROOT="$sdkroot" CC="$cc" \\
        CGO_CFLAGS="-isysroot $sdkroot $min_flag -arch $arch" \\
        CGO_LDFLAGS="-isysroot $sdkroot $min_flag -arch $arch" \\
        go build -ldflags="-s -w" -buildmode=c-archive -o "$out" .
    }

    build_slice iphoneos "-miphoneos-version-min=13.0" arm64 "$OUT/libtailcat_cgo_device.a"
    build_slice iphonesimulator "-mios-simulator-version-min=13.0" arm64 "$OUT/libtailcat_cgo_sim.a"

    # iphoneos and iphonesimulator arm64 slices can't coexist in one fat
    # binary (same arch, different platform), so Xcode's own multi-platform
    # handling is used via a thin per-SDK lipo selection at build time isn't
    # possible with a single vendored_libraries entry -- ship the device
    # slice by default; simulator runs should rebuild with the sim slice.
    # See README.md for how to swap slices when testing on a simulator.
    cp "$OUT/libtailcat_cgo_device.a" "$OUT/libtailcat_cgo.a"
  CMD

  s.dependency 'Flutter'
  s.platform = :ios, '13.0'

  # Flutter.framework does not contain a i386 slice.
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES', 'EXCLUDED_ARCHS[sdk=iphonesimulator*]' => 'i386' }
  s.swift_version = '5.0'
end
