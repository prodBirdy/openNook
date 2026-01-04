fn main() {
    // Add Swift runtime library path for screencapturekit crate
    #[cfg(target_os = "macos")]
    {
        // Get the macOS SDK path (for future use)
        let _sdk_path = std::process::Command::new("xcrun")
            .args(["--sdk", "macosx", "--show-sdk-path"])
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk".to_string());

        // Link to Swift runtime libraries
        println!("cargo:rustc-link-search=/usr/lib/swift");
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

        // Also add Xcode's Swift libraries
        let xcode_swift_path = "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx";
        if std::path::Path::new(xcode_swift_path).exists() {
            println!("cargo:rustc-link-search={}", xcode_swift_path);
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", xcode_swift_path);
        }
    }

    tauri_build::build()
}
