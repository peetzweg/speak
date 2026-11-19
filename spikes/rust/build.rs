//! 1. macOS: link clang's compiler-rt (needed by pyke's static onnxruntime).
//! 2. Feature `espeak-inprocess`: compile espeak-ng 1.52.0 (library + ucd-tools only, no audio,
//!    no mbrola/klatt/async) with `cc` and link it statically. Uses CARGO_CFG_* (the *target*), so
//!    cross-compiling works; no cmake, no bindgen/libclang, no network at build time.
//!    Source comes from scripts/fetch-espeak.sh.

use std::path::PathBuf;

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    // pyke's static onnxruntime for macOS calls `__isPlatformVersionAtLeast` (from `@available`),
    // which lives in clang's compiler-rt; rustc links with -nodefaultlibs, so add it explicitly.
    if target_os == "macos" {
        if let Ok(o) = std::process::Command::new("clang").arg("--print-resource-dir").output() {
            let dir = PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()).join("lib/darwin");
            println!("cargo:rustc-link-search=native={}", dir.display());
            println!("cargo:rustc-link-lib=static=clang_rt.osx");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "espeak-inprocess")]
    espeak(&target_os);
}

#[cfg(feature = "espeak-inprocess")]
fn espeak(target_os: &str) {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("third_party/espeak-ng/src");
    if !root.join("libespeak-ng/speech.c").exists() {
        panic!("espeak-ng source missing: run scripts/fetch-espeak.sh");
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc";

    std::fs::write(
        out.join("config.h"),
        format!(
            "#define HAVE_MKSTEMP {}\n#define USE_ASYNC 0\n#define USE_KLATT 0\n#define USE_LIBPCAUDIO 0\n\
             #define USE_LIBSONIC 0\n#define USE_MBROLA 0\n#define USE_SPEECHPLAYER 0\n#define PACKAGE_VERSION \"1.52.0\"\n{}",
            if target_os == "windows" { 0 } else { 1 },
            if msvc { "#define _CRT_SECURE_NO_WARNINGS\n#define _CRT_NONSTDC_NO_DEPRECATE\n" } else { "" }
        ),
    )
    .unwrap();

    let lib = [
        "common", "mnemonics", "error", "ieee80", "compiledata", "compiledict", "dictionary", "encoding", "intonation",
        "langopts", "numbers", "phoneme", "phonemelist", "readclause", "setlengths", "soundicon", "spect", "ssml",
        "synthdata", "synthesize", "tr_languages", "translate", "translateword", "voices", "wavegen", "speech",
        "espeak_api",
    ];
    let ucd = ["case", "categories", "ctype", "proplist", "scripts", "tostring"];

    // Mirror espeak-ng's own CMake build: NDEBUG, -fwrapv, compat headers (ucd-tools without them).
    let mut b = cc::Build::new();
    b.include(root.join("include"))
        .include(root.join("include/compat"))
        .include(&out)
        .include(root.join("ucd-tools/src/include"))
        .define("NDEBUG", None)
        .define("LIBESPEAK_NG_EXPORT", "1")
        .define("PATH_ESPEAK_DATA", "\"/usr/share/espeak-ng-data\"")
        .warnings(false);
    if !msvc {
        b.flag("-fwrapv").flag("-fvisibility=hidden");
    }
    for f in lib {
        b.file(root.join(format!("libespeak-ng/{f}.c")));
    }
    b.compile("espeak-ng");

    // ucd-tools after espeak-ng: GNU ld resolves static archives in order (macOS ld64 does not care).
    let mut u = cc::Build::new();
    u.include(root.join("ucd-tools/src/include")).define("NDEBUG", None).warnings(false);
    for f in ucd {
        u.file(root.join(format!("ucd-tools/src/{f}.c")));
    }
    u.compile("ucd");

    println!("cargo:rerun-if-changed=third_party/espeak-ng");
}
