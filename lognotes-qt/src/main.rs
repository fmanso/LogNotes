//! LogNotes' desktop entry point.
//!
//! The database path is resolved here — the single OS-specific surface in the codebase — and handed to
//! the journal model, which opens it on its worker thread.
//!
//! With the `driver` feature on, `--livetest` runs the scripted UI scenario in `livetest` instead. That
//! module is compiled out of normal builds, so the shipped app carries neither the driver nor a test
//! dependency.

mod journal;

#[cfg(feature = "driver")]
mod driver;
#[cfg(feature = "driver")]
mod livetest;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

/// The QML entry point inside the compiled resource bundle.
const MAIN_QML: &str = "qrc:/qt/qml/com/lognotes/app/qml/main.qml";

fn main() {
    #[cfg(feature = "driver")]
    if std::env::args().any(|argument| argument == "--livetest") {
        livetest::run();
    }

    let path = database_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    journal::set_database_path(path);

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from(MAIN_QML));
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}

/// Where the notes live on this machine: `%APPDATA%\LogNotes` on Windows,
/// `$XDG_DATA_HOME/lognotes` (or `~/.local/share/lognotes`) everywhere else.
fn database_path() -> std::path::PathBuf {
    if let Some(app_data) = std::env::var_os("APPDATA") {
        return std::path::PathBuf::from(app_data)
            .join("LogNotes")
            .join("lognotes.db");
    }

    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    base.join("lognotes").join("lognotes.db")
}
