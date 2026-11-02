//! Keeping a copy of the database, and keeping a week of them.
//!
//! A backup that has to be remembered is one that does not exist, so the worker writes one as the store
//! opens — at most once a day, before any typing — and drops the oldest beyond a week. The copy itself is
//! `Store::backup_into`, which uses SQLite's own consistent copy rather than reading the file underneath a
//! live connection.

use std::fs;
use std::path::{Path, PathBuf};

use lognotes_core::Store;

/// How many daily copies to keep.
pub const KEEP: usize = 7;

/// The directory the copies live in: beside the database, so backing up is a copy of one folder.
pub fn directory(database: &Path) -> PathBuf {
    database.parent().unwrap_or(Path::new(".")).join("backups")
}

/// Write today's copy if it is not there yet, then drop everything beyond the newest [`KEEP`].
///
/// Returns the copy for today, or `None` when the filesystem would not cooperate — a failed backup is not a
/// reason to refuse to open the notes.
pub fn daily(store: &Store, database: &Path, today: &str) -> Option<PathBuf> {
    let dir = directory(database);
    fs::create_dir_all(&dir).ok()?;

    let path = dir.join(format!("lognotes-{today}.db"));
    if !path.exists() {
        store.backup_into(&path).ok()?;
    }
    prune(&dir);
    Some(path)
}

/// The copies there are, oldest first.
pub fn copies(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("lognotes-") && name.ends_with(".db"))
        })
        .collect();
    // The names are ISO dates, so sorting by name is sorting by age.
    found.sort();
    found
}

/// Drop the oldest copies beyond [`KEEP`]. Best effort: a file that will not go is not an error worth
/// interrupting a launch for.
pub fn prune(dir: &Path) {
    let found = copies(dir);
    let excess = found.len().saturating_sub(KEEP);
    for old in found.into_iter().take(excess) {
        let _ = fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a directory");
        dir
    }

    #[test]
    fn a_day_is_backed_up_once() {
        let dir = temp_dir("lognotes-backup-daily");
        let database = dir.join("lognotes.db");
        let mut store = Store::open(&database).expect("a store");
        store.ensure_page("Something").expect("a page");

        let first = daily(&store, &database, "2026-11-02").expect("a copy");
        let second = daily(&store, &database, "2026-11-02").expect("the same copy");

        assert_eq!(first, second);
        assert_eq!(copies(&dir.join("backups")).len(), 1);
        // The copy is a database in its own right.
        assert!(Store::open(&first).is_ok());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_newest_week_is_kept() {
        let dir = temp_dir("lognotes-backup-prune");
        let backups = dir.join("backups");
        fs::create_dir_all(&backups).expect("a directory");
        for day in 1..=10 {
            fs::write(backups.join(format!("lognotes-2026-11-{day:02}.db")), b"x").expect("a file");
        }

        prune(&backups);

        let kept = copies(&backups);
        assert_eq!(kept.len(), KEEP);
        assert!(
            kept.iter().all(|path| !path.ends_with("lognotes-2026-11-01.db")),
            "the oldest went first: {kept:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pruning_a_missing_directory_is_not_an_error() {
        prune(Path::new("/nonexistent/lognotes/backups"));
    }
}
