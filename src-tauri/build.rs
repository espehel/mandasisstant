use std::{env, fs, path::PathBuf};

/// Minimum macOS version for the whole app. Keep in sync with:
///   - AppleBridge/Package.swift            (platforms: [.macOS(.v26)])
///   - src-tauri/tauri.conf.json            (bundle.macOS.minimumSystemVersion)
///   - src-tauri/.cargo/config.toml         (MACOSX_DEPLOYMENT_TARGET)
///
/// `check_deployment_targets` fails the build if any of them drift.
const MACOS_MIN_VERSION: &str = "26.0";

/// Swift static library product from AppleBridge/Package.swift.
const SWIFT_PRODUCT: &str = "AppleBridgeFFI";
const SWIFT_PACKAGE_DIR: &str = "../AppleBridge";

fn main() {
    check_deployment_targets();

    swift_rs::SwiftLinker::new(MACOS_MIN_VERSION)
        .with_package(SWIFT_PRODUCT, SWIFT_PACKAGE_DIR)
        .link();

    // swift-rs only uses SwiftPM 6.4's new build layout (`<build-path>/<config>` ->
    // `out/Products/<Config>`) when it detects Xcode 27 via `xcodebuild`, which fails
    // when the Command Line Tools are the active developer dir. Add that path ourselves
    // so linking works with either toolchain selected.
    let config = if env::var("DEBUG").as_deref() == Ok("true") {
        "debug"
    } else {
        "release"
    };
    let swift_out = PathBuf::from(env::var("OUT_DIR").unwrap())
        .join("swift-rs")
        .join(SWIFT_PRODUCT)
        .join(config);
    println!("cargo:rustc-link-search=native={}", swift_out.display());

    tauri_build::build()
}

fn check_deployment_targets() {
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

    let env_target = env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_default();
    assert_eq!(
        env_target, MACOS_MIN_VERSION,
        "MACOSX_DEPLOYMENT_TARGET (src-tauri/.cargo/config.toml) must equal MACOS_MIN_VERSION in build.rs"
    );

    let conf: serde_json::Value =
        serde_json::from_str(&fs::read_to_string("tauri.conf.json").unwrap()).unwrap();
    let bundle_min = conf["bundle"]["macOS"]["minimumSystemVersion"].as_str();
    assert_eq!(
        bundle_min,
        Some(MACOS_MIN_VERSION),
        "bundle.macOS.minimumSystemVersion in tauri.conf.json must equal MACOS_MIN_VERSION in build.rs"
    );

    let major = MACOS_MIN_VERSION.split('.').next().unwrap();
    let package_swift = fs::read_to_string(format!("{SWIFT_PACKAGE_DIR}/Package.swift")).unwrap();
    assert!(
        package_swift.contains(&format!(".macOS(.v{major})")),
        "AppleBridge/Package.swift platforms must be .macOS(.v{major}) to match MACOS_MIN_VERSION in build.rs"
    );
}
