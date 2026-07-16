use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=MUKEI_LLAMA_NATIVE_LIB_DIR");
    if env::var_os("CARGO_FEATURE_NATIVE_INFERENCE").is_none() {
        return;
    }

    let directory = env::var_os("MUKEI_LLAMA_NATIVE_LIB_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| {
            panic!(
                "native_inference requires MUKEI_LLAMA_NATIVE_LIB_DIR pointing to the ABI-matched llama capsule"
            )
        });
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let (prefix, suffix) = match target_os.as_str() {
        "windows" => ("", ".dll"),
        "macos" | "ios" => ("lib", ".dylib"),
        _ => ("lib", ".so"),
    };
    let library = directory.join(format!("{prefix}mukei_llama_native{suffix}"));
    if !library.is_file() {
        panic!(
            "native inference capsule is missing at {}",
            library.display()
        );
    }

    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=dylib=mukei_llama_native");
}
