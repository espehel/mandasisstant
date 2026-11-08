// swift-tools-version: 6.2
import PackageDescription

// Deployment target: macOS 26.0. Keep in sync with
//   - src-tauri/build.rs            (MACOS_MIN_VERSION, passed to SwiftLinker)
//   - src-tauri/tauri.conf.json     (bundle.macOS.minimumSystemVersion)
//   - src-tauri/.cargo/config.toml  (MACOSX_DEPLOYMENT_TARGET)
// src-tauri/build.rs fails the build if these drift apart.
let package = Package(
    name: "AppleBridge",
    platforms: [.macOS(.v26)],
    products: [
        // Static library linked into the Tauri app by swift-rs (SwiftLinker).
        // The product name must match the name passed to `with_package` in build.rs.
        .library(name: "AppleBridgeFFI", type: .static, targets: ["AppleBridgeFFI"]),
        .executable(name: "bridge-test", targets: ["bridge-test"]),
    ],
    dependencies: [
        // Swift half of swift-rs (SRString etc.). Keep the version identical to the
        // `swift-rs` crate in src-tauri/Cargo.toml.
        .package(url: "https://github.com/Brendonovich/swift-rs", exact: "1.0.9"),
    ],
    targets: [
        // Translation and microphone-permission logic, as a plain async Swift API. No FFI concerns.
        .target(name: "AppleBridgeCore"),

        // Thin @_cdecl wrappers: convert types and forward to AppleBridgeCore.
        .target(
            name: "AppleBridgeFFI",
            dependencies: [
                "AppleBridgeCore",
                .product(name: "SwiftRs", package: "swift-rs"),
            ]
        ),

        // CLI that exercises AppleBridgeCore directly, without Tauri.
        .executableTarget(name: "bridge-test", dependencies: ["AppleBridgeCore"]),
    ]
)
