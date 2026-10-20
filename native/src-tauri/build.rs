fn main() {
    println!("cargo:rerun-if-changed=macos/AudioCapture.swift");
    println!("cargo:rerun-if-changed=macos/CaptureInfo.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let target = std::env::var("TARGET").unwrap();
        let arch = if target.starts_with("aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        let status = std::process::Command::new("xcrun")
            .args([
                "swiftc",
                "-parse-as-library",
                "-O",
                "-target",
                &format!("{arch}-apple-macosx13.3"),
                "macos/AudioCapture.swift",
                "-o",
            ])
            .arg(out.join("golos-capture"))
            .args([
                "-Xlinker",
                "-sectcreate",
                "-Xlinker",
                "__TEXT",
                "-Xlinker",
                "__info_plist",
                "-Xlinker",
                "macos/CaptureInfo.plist",
            ])
            .status()
            .expect("Swift compiler is required for macOS audio capture");
        assert!(
            status.success(),
            "Could not compile ScreenCaptureKit audio capture"
        );
    }
    tauri_build::build()
}
