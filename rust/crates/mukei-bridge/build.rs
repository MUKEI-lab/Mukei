fn main() {
    // CXX-Qt's CMake wrapper addresses this Rust library target as
    // `mukei_bridge`, while Cargo exposes the package name as `mukei-bridge`.
    // cxx-qt-build 0.9.x derives its export directory from CARGO_PKG_NAME, so
    // normalize that value only for the CMake-export path. Normal Cargo and
    // Android builds do not set this export flag and retain the package name.
    println!("cargo:rerun-if-env-changed=CXX_QT_EXPORT_CRATE_mukei_bridge");
    if std::env::var_os("CXX_QT_EXPORT_CRATE_mukei_bridge").is_some() {
        std::env::set_var("CARGO_PKG_NAME", "mukei_bridge");
    }

    cxx_qt_build::CxxQtBuilder::new()
        .file("src/lib.rs")
        .build();
}
