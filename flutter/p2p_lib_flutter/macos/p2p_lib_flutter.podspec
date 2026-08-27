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

  # No Objective-C/Swift/C sources of our own; the actual implementation is
  # the Go cgo shared library built by the script phase below.
  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'

  s.vendored_libraries = 'libtailcat_cgo.dylib'

  # Build the Go cgo shared library for the host (macOS is always built
  # natively, never cross-compiled from another host) and drop it where
  # vendored_libraries expects to find it, before CocoaPods packages the pod.
  # Go's `-buildmode=c-shared` stamps the dylib's LC_ID_DYLIB (install name)
  # as the bare filename "libtailcat_cgo.dylib" instead of an @rpath-style
  # path. Left as-is, the app binary that links against it also records
  # that bare name as the dependency to resolve at launch, and dyld's
  # default search (cwd, DYLD_LIBRARY_PATH, ...) doesn't include
  # Contents/Frameworks -- so the executable fails to launch with "Library
  # not loaded: libtailcat_cgo.dylib" even though the file is right there
  # next to it. Rewriting the install name to @rpath/... makes it resolve
  # via the `@executable_path/../Frameworks` rpath Xcode already adds.
  s.prepare_command = <<-CMD
    set -e
    cd "#{File.dirname(__FILE__)}/../../../tailcat-cgo"
    OUT="#{File.dirname(__FILE__)}/libtailcat_cgo.dylib"
    CGO_ENABLED=1 go build -buildmode=c-shared -o "$OUT" .
    install_name_tool -id "@rpath/libtailcat_cgo.dylib" "$OUT"
  CMD

  s.dependency 'FlutterMacOS'

  s.platform = :osx, '10.11'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
  s.swift_version = '5.0'
end
