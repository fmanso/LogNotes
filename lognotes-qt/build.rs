use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let mut builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("com.lognotes.app").qml_file("qml/main.qml"),
    )
    .qt_module("Quick")
    .qt_module("Gui")
    .files(["src/journal.rs"])
    // Styling a block's markdown needs a QTextCharFormat, which only C++ can hand a text document.
    .cpp_file("markdown_shim.cpp");

    // The live-test driver is development-only: without this feature the app links no QtTest and
    // carries no test hooks.
    if std::env::var_os("CARGO_FEATURE_DRIVER").is_some() {
        builder = builder
            .files(["src/driver.rs"])
            .cpp_file("driver_shim.cpp");
    }

    builder.build();
}
