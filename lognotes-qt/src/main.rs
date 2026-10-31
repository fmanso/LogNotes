//! LogNotes' desktop entry point.
//!
//! The database path is resolved here — the single OS-specific surface in the codebase — and handed to
//! the journal model, which opens it on its worker thread. A file that will not open surfaces as the
//! error screen rather than as an empty journal.

mod journal;

use std::path::PathBuf;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

fn main() {
    let path = database_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    journal::set_database_path(path);

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/com/lognotes/app/qml/main.qml"));
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}

fn database_path() -> PathBuf {
    if let Some(app_data) = std::env::var_os("APPDATA") {
        return PathBuf::from(app_data).join("LogNotes").join("lognotes.db");
    }

    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("lognotes").join("lognotes.db")
}
