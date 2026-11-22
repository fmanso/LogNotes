//! A small file log beside the database.
//!
//! The app is a GUI on platforms where a startup failure has no console. A log that names the build, the
//! platform, the data path, and the last thing that failed is the difference between a report and a guess.
//! Logging is best effort: a file that cannot be opened never prevents the notes from opening.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use chrono::Local;

/// Rotate at a megabyte; a long-running install keeps three files at most.
const LIMIT: u64 = 1024 * 1024;

static LOG: OnceLock<Mutex<Option<File>>> = OnceLock::new();

/// Open the log, rotate it if needed, and install a panic hook that writes before the process exits.
pub fn init(log: &Path, data: &Path) {
    let file = open_log(log);
    let _ = LOG.set(Mutex::new(file));

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = if let Some(text) = info.payload().downcast_ref::<&str>() {
            (*text).to_string()
        } else if let Some(text) = info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            "panic with a non-string payload".to_string()
        };
        let location = info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        error(&format!(
            "panic at {location}: {message}\n{}",
            std::backtrace::Backtrace::force_capture()
        ));
        previous(info);
    }));

    info(&header(data));
}

/// The first line of every log: what is running, where, and on what.
fn header(data: &Path) -> String {
    format!(
        "LogNotes {} on {}; data {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        data.display()
    )
}

pub fn info(message: &str) {
    write("INFO", message);
}

pub fn warn(message: &str) {
    write("WARN", message);
}

pub fn error(message: &str) {
    write("ERROR", message);
}

fn write(level: &str, message: &str) {
    let Some(lock) = LOG.get() else {
        return;
    };
    let Ok(mut guard) = lock.lock() else {
        return;
    };
    if let Some(file) = guard.as_mut() {
        let _ = writeln!(
            file,
            "{} {level} {message}",
            Local::now().format("%Y-%m-%dT%H:%M:%S")
        );
        let _ = file.flush();
    }
}

/// Open the current log in append mode, rotating first when it is over the limit.
fn open_log(path: &Path) -> Option<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    let _ = rotate(path);
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// Move `lognotes.log` to `lognotes.1.log`, shifting the old one to `.2` and dropping a third.
fn rotate(path: &Path) -> std::io::Result<()> {
    let Ok(meta) = fs::metadata(path) else {
        return Ok(());
    };
    if meta.len() < LIMIT {
        return Ok(());
    }

    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("lognotes");
    let second = path.with_file_name(format!("{stem}.2.log"));
    let first = path.with_file_name(format!("{stem}.1.log"));
    let _ = fs::remove_file(&second);
    if first.exists() {
        fs::rename(&first, &second)?;
    }
    fs::rename(path, &first)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lognotes-diagnostics-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a directory");
        path
    }

    #[test]
    fn a_log_that_passes_the_limit_rotates_and_keeps_three() {
        let dir = temp("rotate");
        let log = dir.join("lognotes.log");
        fs::write(&log, vec![b'x'; (LIMIT + 1) as usize]).expect("a large log");

        rotate(&log).expect("the first rotation");
        assert!(dir.join("lognotes.1.log").is_file());
        assert!(!log.exists());

        fs::write(&log, vec![b'y'; (LIMIT + 1) as usize]).expect("a second large log");
        rotate(&log).expect("the second rotation");
        assert!(dir.join("lognotes.2.log").is_file());
        assert!(dir.join("lognotes.1.log").is_file());

        // A third rotation drops the oldest and leaves exactly three files once the new current log is
        // opened.
        fs::write(&log, vec![b'z'; (LIMIT + 1) as usize]).expect("a third large log");
        rotate(&log).expect("the third rotation");
        fs::write(&log, b"new log").expect("the new current log");
        let files = fs::read_dir(&dir)
            .expect("the directory")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_file())
            .count();
        assert_eq!(files, 3, "current, .1, .2");
        assert!(dir.join("lognotes.2.log").is_file());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_small_log_is_not_rotated() {
        let dir = temp("small");
        let log = dir.join("lognotes.log");
        fs::write(&log, b"one line").expect("a log");
        rotate(&log).expect("no rotation");
        assert!(log.is_file());
        assert!(!dir.join("lognotes.1.log").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_a_missing_log_creates_it() {
        let dir = temp("missing");
        let log = dir.join("lognotes.log");
        let file = open_log(&log).expect("a new log");
        drop(file);
        assert!(log.is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_header_names_the_build_the_platform_and_the_data_path() {
        let line = header(Path::new("/home/person/.local/share/lognotes/lognotes.db"));
        assert!(line.contains(env!("CARGO_PKG_VERSION")));
        assert!(line.contains(std::env::consts::OS));
        assert!(line.contains("/home/person/.local/share/lognotes/lognotes.db"));
    }

    #[test]
    fn an_unusable_log_path_returns_no_file() {
        let dir = temp("unusable");
        let blocker = dir.join("not-a-directory");
        fs::write(&blocker, b"x").expect("the blocker");
        assert!(open_log(&blocker.join("lognotes.log")).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
