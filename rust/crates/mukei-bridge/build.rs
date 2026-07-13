use std::path::PathBuf;

fn main() {
    // CXX-Qt's CMake wrapper addresses this Rust library target as
    // `mukei_bridge`, while Cargo exposes the package name as `mukei-bridge`.
    // cxx-qt-build 0.9.x derives its export directory from CARGO_PKG_NAME, so
    // normalize that value only for the CMake-export path. Normal Cargo and
    // Android builds do not set this export flag and retain the package name.
    println!("cargo:rerun-if-env-changed=CXX_QT_EXPORT_CRATE_mukei_bridge");
    let cmake_export = std::env::var_os("CXX_QT_EXPORT_CRATE_mukei_bridge").is_some();
    if cmake_export {
        std::env::set_var("CARGO_PKG_NAME", "mukei_bridge");
    }

    cxx_qt_build::CxxQtBuilder::new()
        .file("src/lib.rs")
        .build();

    if cmake_export {
        // The existing C++ entry point intentionally keeps the Cargo package
        // include contract (`mukei-bridge/...`). The official CMake wrapper,
        // however, exports this target under the sanitized CMake identity
        // (`mukei_bridge/...`). Mirror only the two generated bridge headers
        // into a compatibility prefix inside the same exported include root.
        // This keeps desktop CMake integration compatible without changing
        // Android/raw-Cargo header discovery.
        let export_dir = PathBuf::from(
            std::env::var_os("CXX_QT_EXPORT_DIR")
                .expect("CXX_QT_EXPORT_DIR must be set for a CMake CXX-Qt export"),
        );
        let include_root = export_dir
            .join("crates")
            .join("mukei_bridge")
            .join("include");
        let generated_dir = include_root.join("mukei_bridge").join("src");
        let compatibility_dir = include_root.join("mukei-bridge").join("src");

        std::fs::create_dir_all(&compatibility_dir)
            .expect("failed to create CXX-Qt compatibility include directory");

        for header in ["lib.cxxqt.h", "lib.cxx.h"] {
            let source = generated_dir.join(header);
            let destination = compatibility_dir.join(header);
            std::fs::copy(&source, &destination).unwrap_or_else(|error| {
                panic!(
                    "failed to mirror generated CXX-Qt header {} to {}: {error}",
                    source.display(),
                    destination.display()
                )
            });
        }
    }
}
