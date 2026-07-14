use std::path::{Path, PathBuf};

fn default_native_lib_dir(target: &str) -> PathBuf {
    let abi = match target {
        "aarch64-linux-android" => "arm64-v8a",
        "armv7-linux-androideabi" => "armeabi-v7a",
        "x86_64-linux-android" => "x86_64",
        _ => "host",
    };
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../llama-cpp-prebuilt/prebuilt")
        .join(abi)
}

fn configure_native_inference_link() {
    if std::env::var_os("CARGO_FEATURE_LLAMA_CPP").is_none() {
        return;
    }
    println!("cargo:rerun-if-env-changed=MUKEI_LLAMA_NATIVE_LIB_DIR");
    let target = std::env::var("TARGET").expect("Cargo always provides TARGET");
    let dir = std::env::var_os("MUKEI_LLAMA_NATIVE_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_native_lib_dir(&target));
    let library = if target.contains("apple") {
        dir.join("libmukei_llama_native.dylib")
    } else if target.contains("windows") {
        dir.join("mukei_llama_native.dll")
    } else {
        dir.join("libmukei_llama_native.so")
    };
    if !library.is_file() {
        panic!(
            "mukei-bridge/llama_cpp requires the native inference capsule: {}",
            library.display()
        );
    }
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=dylib=mukei_llama_native");
}

fn main() {
    configure_native_inference_link();
    cxx_qt_build::CxxQtBuilder::new().file("src/lib.rs").build();
}
