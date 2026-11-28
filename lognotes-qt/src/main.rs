//! LogNotes' desktop entry point.
//!
//! The database path is resolved here — the single OS-specific surface in the codebase — and handed to
//! the journal model, which opens it on its worker thread.
//!
//! With the `driver` feature on, `--livetest` runs the scripted UI scenario in `livetest` instead. That
//! module is compiled out of normal builds, so the shipped app carries neither the driver nor a test
//! dependency.

mod agent;
mod backup;
mod bridge;
mod diagnostics;
mod diagram;
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
    {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|argument| argument == "--livetest") {
            livetest::run();
        }
        if args.iter().any(|argument| argument == "--livetest-error") {
            livetest::run_error();
        }
    }

    let path = database_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let log = path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("lognotes.log");
    diagnostics::init(&log, &path);
    diagnostics::info(&format!("starting; database {}", path.display()));

    if cfg!(windows) {
        if let Some((old, new)) = windows_data_dirs() {
            if old.join("lognotes.db").exists() && !new.join("lognotes.db").exists() {
                match migrate_windows_graph(&old, &new) {
                    Ok(()) => diagnostics::info(&format!(
                        "moved the roaming graph from {} to {}",
                        old.display(),
                        new.display()
                    )),
                    Err(error) => {
                        diagnostics::error(&format!(
                            "could not move the roaming graph from {} to {}: {error}",
                            old.display(),
                            new.display()
                        ));
                        journal::set_startup_error(format!(
                            "could not move the notes from {} to {}: {error}",
                            old.display(),
                            new.display()
                        ));
                    }
                }
            }
        }
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

/// Where the notes live on this machine: `%LOCALAPPDATA%\LogNotes` on Windows,
/// `$XDG_DATA_HOME/lognotes` (or `~/.local/share/lognotes`) everywhere else.
fn database_path() -> std::path::PathBuf {
    if cfg!(windows) {
        if let Some(path) = windows_database_path(
            std::env::var_os("LOCALAPPDATA").as_deref(),
            std::env::var_os("APPDATA").as_deref(),
        ) {
            return path;
        }
    }

    linux_database_path(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

fn windows_database_path(
    local: Option<&std::ffi::OsStr>,
    roaming: Option<&std::ffi::OsStr>,
) -> Option<std::path::PathBuf> {
    local.or(roaming).map(|base| {
        std::path::PathBuf::from(base)
            .join("LogNotes")
            .join("lognotes.db")
    })
}

fn linux_database_path(
    xdg: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> std::path::PathBuf {
    let base = xdg
        .map(std::path::PathBuf::from)
        .or_else(|| {
            home.map(|home| {
                std::path::PathBuf::from(home)
                    .join(".local")
                    .join("share")
            })
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("lognotes").join("lognotes.db")
}

/// The v0.1.0 roaming folder and the local folder that replaces it, when this machine has both roots
/// and they are different.
fn windows_data_dirs() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let local = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("LogNotes");
    let roaming = std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("LogNotes");
    (local != roaming).then_some((roaming, local))
}

/// Move a v0.1.0 graph from `%APPDATA%\LogNotes` to `%LOCALAPPDATA%\LogNotes` once.
///
/// The database, its WAL sidecars, and the backup files move together. If the sidecars cannot move,
/// nothing moves; if the database cannot move after them, they are put back. A failure here must not
/// start an empty graph, so it is returned to the caller.
fn migrate_windows_graph(old: &std::path::Path, new: &std::path::Path) -> std::io::Result<()> {
    let old_db = old.join("lognotes.db");
    let new_db = new.join("lognotes.db");
    if new_db.exists() || !old_db.exists() {
        return Ok(());
    }

    std::fs::create_dir_all(new)?;

    let mut moved = Vec::new();
    for name in ["lognotes.db-wal", "lognotes.db-shm"] {
        let from = old.join(name);
        if from.exists() {
            let to = new.join(name);
            move_or_copy(&from, &to)?;
            moved.push(name);
        }
    }
    if let Err(error) = move_or_copy(&old_db, &new_db) {
        for name in moved {
            let _ = move_or_copy(&new.join(name), &old.join(name));
        }
        return Err(error);
    }

    // The graph is usable even if a backup file will not move; do not fail the launch for it.
    let old_backups = old.join("backups");
    if old_backups.is_dir() {
        let new_backups = new.join("backups");
        let _ = std::fs::create_dir_all(&new_backups);
        if let Ok(entries) = std::fs::read_dir(&old_backups) {
            for entry in entries.filter_map(|entry| entry.ok()) {
                let from = entry.path();
                if from.is_file() {
                    let Some(name) = from.file_name() else {
                        continue;
                    };
                    let _ = move_or_copy(&from, &new_backups.join(name));
                }
            }
        }
    }
    Ok(())
}

fn move_or_copy(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_then_remove(from, to)
}

fn copy_then_remove(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lognotes-path-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory");
        path
    }

    #[test]
    fn windows_prefers_local_app_data_and_falls_back_to_roaming() {
        let local = windows_database_path(Some(std::ffi::OsStr::new(r"C:\Local")), Some(std::ffi::OsStr::new(r"C:\Roaming")))
            .expect("a path");
        assert_eq!(local, PathBuf::from(r"C:\Local").join("LogNotes").join("lognotes.db"));
        let roaming = windows_database_path(None, Some(std::ffi::OsStr::new(r"C:\Roaming")))
            .expect("a path");
        assert_eq!(roaming, PathBuf::from(r"C:\Roaming").join("LogNotes").join("lognotes.db"));
        assert!(windows_database_path(None, None).is_none());
    }

    #[test]
    fn linux_uses_xdg_then_home_then_the_current_directory() {
        assert_eq!(
            linux_database_path(Some(std::ffi::OsStr::new("/data")), None),
            PathBuf::from("/data").join("lognotes").join("lognotes.db")
        );
        assert_eq!(
            linux_database_path(None, Some(std::ffi::OsStr::new("/home/person"))),
            PathBuf::from("/home/person")
                .join(".local")
                .join("share")
                .join("lognotes")
                .join("lognotes.db")
        );
        assert_eq!(
            linux_database_path(None, None),
            PathBuf::from(".").join("lognotes").join("lognotes.db")
        );
    }

    #[test]
    fn a_roaming_graph_moves_with_its_sidecars_and_backups() {
        let root = temp("move");
        let old = root.join("roaming");
        let new = root.join("local");
        std::fs::create_dir_all(old.join("backups")).expect("old");
        std::fs::write(old.join("lognotes.db"), b"graph").expect("db");
        std::fs::write(old.join("lognotes.db-wal"), b"wal").expect("wal");
        std::fs::write(old.join("backups").join("lognotes-2026-11-02.db"), b"copy").expect("copy");

        migrate_windows_graph(&old, &new).expect("the move");

        assert_eq!(std::fs::read(new.join("lognotes.db")).expect("db"), b"graph");
        assert_eq!(std::fs::read(new.join("lognotes.db-wal")).expect("wal"), b"wal");
        assert_eq!(
            std::fs::read(new.join("backups").join("lognotes-2026-11-02.db")).expect("copy"),
            b"copy"
        );
        assert!(!old.join("lognotes.db").exists(), "the old graph moved");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_local_graph_wins_when_both_exist() {
        let root = temp("both");
        let old = root.join("roaming");
        let new = root.join("local");
        std::fs::create_dir_all(&old).expect("old");
        std::fs::create_dir_all(&new).expect("new");
        std::fs::write(old.join("lognotes.db"), b"roaming").expect("old db");
        std::fs::write(new.join("lognotes.db"), b"local").expect("new db");

        migrate_windows_graph(&old, &new).expect("a no-op");

        assert_eq!(std::fs::read(new.join("lognotes.db")).expect("new"), b"local");
        assert_eq!(std::fs::read(old.join("lognotes.db")).expect("old"), b"roaming");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_move_that_cannot_create_the_target_leaves_the_old_graph() {
        let root = temp("move-failure");
        let old = root.join("roaming");
        std::fs::create_dir_all(&old).expect("old");
        std::fs::write(old.join("lognotes.db"), b"graph").expect("db");
        let blocker = root.join("not-a-directory");
        std::fs::write(&blocker, b"x").expect("the blocker");
        let new = blocker.join("local");

        assert!(migrate_windows_graph(&old, &new).is_err());
        assert!(
            old.join("lognotes.db").exists(),
            "the old graph is still there"
        );
        assert!(!new.exists(), "no empty local graph was made");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_copy_fallback_copies_and_removes() {
        let root = temp("copy-fallback");
        let from = root.join("from.db");
        let to = root.join("to.db");
        std::fs::write(&from, b"data").expect("from");
        copy_then_remove(&from, &to).expect("the fallback");
        assert_eq!(std::fs::read(&to).expect("to"), b"data");
        assert!(!from.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
