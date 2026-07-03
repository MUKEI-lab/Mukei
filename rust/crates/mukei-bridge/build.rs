fn main() {
    cxx_qt_build::CxxQtBuilder::new()
        .file("src/lib.rs")
        .build();
}
