//! pyke's static ONNX Runtime for macOS calls `__isPlatformVersionAtLeast` (from `@available`),
//! which lives in clang's compiler runtime. rustc links with `-nodefaultlibs`, so add it here.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    if let Ok(out) = std::process::Command::new("clang").arg("--print-resource-dir").output() {
        let dir = std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()).join("lib/darwin");
        println!("cargo:rustc-link-search=native={}", dir.display());
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    }
}
