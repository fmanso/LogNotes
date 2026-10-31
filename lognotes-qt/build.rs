use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("com.lognotes.app").qml_file("qml/main.qml"),
    )
    .qt_module("Quick")
    .files(["src/journal.rs"])
    .build();
}
