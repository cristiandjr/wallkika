use std::{env, process::Command};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_macos_screensaver();
    }
    tauri_build::build()
}

fn build_macos_screensaver() {
    for path in [
        "../screensaver/macos",
        "../scripts/build-saver.sh",
        "target/screensaver/WallKika.saver/Contents/MacOS/WallKika",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let status = Command::new("bash")
        .args(["../scripts/build-saver.sh", &version])
        .status()
        .expect("bash is needed to build the macOS screen saver");
    assert!(status.success(), "Could not build the macOS screen saver");
}
