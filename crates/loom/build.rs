fn main() {
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        && std::env::var_os("CARGO_FEATURE_MACOS_APP_INTENTS").is_some()
    {
        build_app_intents();
    }
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../../assets/icons/icon.ico");
        res.compile().expect("failed to compile windows resources");
    }
}

fn build_app_intents() {
    use std::{env, path::PathBuf, process::Command};
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../dist/macos/app-intents");
    for file in [
        "build.sh",
        "Bridge.swift",
        "Intents.swift",
        "const-gather.json",
    ] {
        println!("cargo:rerun-if-changed={}", source.join(file).display());
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => panic!("unsupported macOS architecture: {other}"),
    };
    assert!(
        Command::new("bash")
            .arg(source.join("build.sh"))
            .arg(&output)
            .arg(arch)
            .status()
            .expect("run Swift compiler")
            .success(),
        "could not compile App Intents (requires Swift 6 / Xcode 16 or newer)"
    );
    println!("cargo:rustc-link-search=native={}", output.display());
    // App Intents discovers Swift protocol conformances at runtime. Preserve
    // all of them, including types with no direct reference from Rust.
    println!("cargo:rustc-link-lib=static:+whole-archive=LoomAppIntents");
    let swift = Command::new("xcrun")
        .args(["--find", "swiftc"])
        .output()
        .expect("find swiftc");
    assert!(swift.status.success(), "xcrun could not find swiftc");
    let compiler = PathBuf::from(String::from_utf8(swift.stdout).unwrap().trim());
    let runtime = compiler.parent().unwrap().join("../lib/swift/macosx");
    println!("cargo:rustc-link-search=native={}", runtime.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
}
