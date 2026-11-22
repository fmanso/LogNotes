//! Keeping a copy of the database, and keeping a week of them.
//!
//! A backup that has to be remembered is one that does not exist, so the worker writes one as the store
//! opens — at most once a day, before any typing — and drops the oldest beyond a week. The copy itself is
//! `Store::backup_into`, which uses SQLite's own consistent copy rather than reading the file underneath a
//! live connection.

use std::fs;
use std::path::{Path, PathBuf};

use lognotes_core::{Error, Result, Store};

/// How many daily and manual copies to keep.
pub const DAILY_KEEP: usize = 7;
pub const MANUAL_KEEP: usize = 7;
/// How many safety copies of each kind to keep.
pub const SAFETY_KEEP: usize = 3;

/// What a file in the backups folder is for. Retention is per kind so a burst of manual copies cannot
/// evict the daily history or the pre-migration safety net.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyKind {
    Daily,
    Manual,
    PreMigration,
    PreRestore,
}

impl CopyKind {
    pub fn label(self) -> &'static str {
        match self {
            CopyKind::Daily => "daily",
            CopyKind::Manual => "manual",
            CopyKind::PreMigration => "pre-migration",
            CopyKind::PreRestore => "pre-restore",
        }
    }
}

/// One file in the backups folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub path: PathBuf,
    pub kind: CopyKind,
}

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

/// Write a copy now, named with the time so it does not collide with the day's automatic copy.
pub fn now(store: &Store, database: &Path) -> Option<PathBuf> {
    let dir = directory(database);
    fs::create_dir_all(&dir).ok()?;

    let stamp = chrono::Local::now().format("%Y-%m-%dT%H%M%S").to_string();
    let path = dir.join(format!("lognotes-{stamp}.db"));
    store.backup_into(&path).ok()?;
    prune(&dir);
    Some(path)
}

/// The copies there are, oldest first, each with what it is for.
pub fn copies(dir: &Path) -> Vec<Copy> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<Copy> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            let kind = kind_of(name)?;
            Some(Copy { path, kind })
        })
        .collect();
    // The names are ISO dates and times, so sorting by name is sorting by age.
    found.sort_by(|left, right| left.path.cmp(&right.path));
    found
}

fn kind_of(name: &str) -> Option<CopyKind> {
    if !name.ends_with(".db") {
        return None;
    }
    if name.starts_with("pre-migration-") {
        return Some(CopyKind::PreMigration);
    }
    if name.starts_with("pre-restore-") {
        return Some(CopyKind::PreRestore);
    }
    if let Some(rest) = name.strip_prefix("lognotes-") {
        // `lognotes-2026-11-02.db` is a day; the manual copy adds `T202538`.
        return Some(if rest.contains('T') {
            CopyKind::Manual
        } else {
            CopyKind::Daily
        });
    }
    None
}

/// Drop the oldest copies beyond each kind's limit. Best effort: a file that will not go is not an
/// error worth interrupting a launch for.
pub fn prune(dir: &Path) {
    let found = copies(dir);
    for kind in [
        CopyKind::Daily,
        CopyKind::Manual,
        CopyKind::PreMigration,
        CopyKind::PreRestore,
    ] {
        let limit = match kind {
            CopyKind::Daily => DAILY_KEEP,
            CopyKind::Manual => MANUAL_KEEP,
            CopyKind::PreMigration | CopyKind::PreRestore => SAFETY_KEEP,
        };
        let mut of_kind: Vec<&Copy> = found.iter().filter(|copy| copy.kind == kind).collect();
        let excess = of_kind.len().saturating_sub(limit);
        for old in of_kind.drain(..excess) {
            let _ = fs::remove_file(&old.path);
        }
    }
}

/// A restore that has been validated but not yet applied.
///
/// The live connection still owns the database when this is built, so the caller drops its store before
/// calling [`Restore::apply`]. A validation failure happens before the live files are touched.
#[derive(Debug)]
pub struct Restore {
    live: PathBuf,
    staged: PathBuf,
    pre_restore: PathBuf,
    had_store: bool,
}

impl Restore {
    /// Where the current graph was saved, or will be moved, before the swap.
    pub fn pre_restore(&self) -> &Path {
        &self.pre_restore
    }

    /// Replace the live files with the validated copy. Returns the pre-restore path.
    pub fn apply(self) -> Result<PathBuf> {
        if !self.had_store {
            // The live file could not be opened, so there was no connection to vacuum. Keep the
            // unopenable file and its sidecars beside the backups rather than deleting them.
            move_aside(&self.live, &self.pre_restore)?;
            for suffix in ["-wal", "-shm"] {
                let from = sidecar(&self.live, suffix);
                if from.exists() {
                    let to = sidecar(&self.pre_restore, suffix);
                    let _ = move_aside(&from, &to);
                }
            }
        }

        for path in [
            self.live.clone(),
            sidecar(&self.live, "-wal"),
            sidecar(&self.live, "-shm"),
        ] {
            if path.exists() {
                fs::remove_file(&path).map_err(|error| Error::Backup(error.to_string()))?;
            }
        }

        if let Err(error) = fs::rename(&self.staged, &self.live) {
            if self.had_store {
                let _ = fs::copy(&self.pre_restore, &self.live);
            } else {
                let _ = move_aside(&self.pre_restore, &self.live);
            }
            return Err(Error::Backup(error.to_string()));
        }
        Ok(self.pre_restore)
    }
}

/// Write the pre-restore copy, stage the chosen file, and validate it without touching the live files.
///
/// `store` is the open graph, or `None` when the live database could not be opened. In the second case
/// the pre-restore copy is the unopenable live file, moved aside by [`Restore::apply`].
pub fn restore_database(
    store: Option<&Store>,
    live: &Path,
    chosen: &Path,
    backups: &Path,
) -> Result<Restore> {
    if !chosen.is_file() {
        return Err(Error::Backup(format!(
            "{} is not a file",
            chosen.display()
        )));
    }
    if same_file(live, chosen) {
        return Err(Error::Backup("a database cannot be restored over itself".to_string()));
    }

    fs::create_dir_all(backups).map_err(|error| Error::Backup(error.to_string()))?;
    let stamp = chrono::Local::now()
        .format("%Y-%m-%dT%H%M%S")
        .to_string();
    let pre_restore = backups.join(format!(
        "pre-restore-{stamp}-{}.db",
        std::process::id()
    ));

    if let Some(store) = store {
        store.backup_into(&pre_restore)?;
    }

    let name = live
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::Backup(format!("{} has no file name", live.display())))?;
    let staged = live.with_file_name(format!("{name}.restoring"));
    if staged.exists() {
        fs::remove_file(&staged).map_err(|error| Error::Backup(error.to_string()))?;
    }
    fs::copy(chosen, &staged).map_err(|error| Error::Backup(error.to_string()))?;

    // Open the copy before anything live is removed. This also migrates a backup made by an older
    // version, so the swap always installs a usable file.
    match Store::open_with_migration_backup(&staged, backups) {
        Ok(candidate) => drop(candidate),
        Err(error) => {
            let _ = fs::remove_file(&staged);
            return Err(Error::Backup(format!(
                "{} is not a usable database: {error}",
                chosen.display()
            )));
        }
    }

    Ok(Restore {
        live: live.to_path_buf(),
        staged,
        pre_restore,
        had_store: store.is_some(),
    })
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{name}{suffix}"))
}

fn move_aside(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|error| Error::Backup(error.to_string()))?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to).map_err(|error| Error::Backup(error.to_string()))?;
    fs::remove_file(from).map_err(|error| Error::Backup(error.to_string()))?;
    Ok(())
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
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
    fn a_manual_copy_sits_beside_the_daily_one() {
        let dir = temp_dir("lognotes-backup-manual");
        let database = dir.join("lognotes.db");
        let mut store = Store::open(&database).expect("a store");
        store.ensure_page("Something").expect("a page");

        daily(&store, &database, "2026-11-02").expect("a daily copy");
        let manual = now(&store, &database).expect("a copy now");

        let names: Vec<String> = copies(&dir.join("backups"))
            .iter()
            .map(|copy| copy.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        // The day's copy still sorts first, and the manual one is not mistaken for another day.
        assert_eq!(names[0], "lognotes-2026-11-02.db");
        assert!(manual.exists());

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
        assert_eq!(kept.len(), DAILY_KEEP);
        assert!(
            kept.iter().all(|copy| !copy.path.ends_with("lognotes-2026-11-01.db")),
            "the oldest went first: {kept:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manual_and_safety_copies_have_their_own_pools() {
        let dir = temp_dir("lognotes-backup-pools");
        let backups = dir.join("backups");
        fs::create_dir_all(&backups).expect("a directory");
        for day in 1..=9 {
            fs::write(backups.join(format!("lognotes-2026-11-{day:02}.db")), b"d").expect("daily");
            fs::write(
                backups.join(format!("lognotes-2026-11-{day:02}T120000.db")),
                b"m",
            )
            .expect("manual");
        }
        for step in 1..=5 {
            fs::write(
                backups.join(format!("pre-migration-v{step}-to-v3-2026-11-02T120000.db")),
                b"p",
            )
            .expect("migration");
            fs::write(
                backups.join(format!("pre-restore-2026-11-0{step}T120000.db")),
                b"r",
            )
            .expect("restore");
        }

        prune(&backups);

        let kept = copies(&backups);
        let count = |kind: CopyKind| kept.iter().filter(|copy| copy.kind == kind).count();
        assert_eq!(count(CopyKind::Daily), DAILY_KEEP);
        assert_eq!(count(CopyKind::Manual), MANUAL_KEEP);
        assert_eq!(count(CopyKind::PreMigration), SAFETY_KEEP);
        assert_eq!(count(CopyKind::PreRestore), SAFETY_KEEP);
        // A manual burst did not take the daily history with it.
        assert!(kept
            .iter()
            .any(|copy| copy.kind == CopyKind::Daily && copy.path.ends_with("lognotes-2026-11-09.db")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_are_classified_by_their_names() {
        assert_eq!(kind_of("lognotes-2026-11-02.db"), Some(CopyKind::Daily));
        assert_eq!(kind_of("lognotes-2026-11-02T120000.db"), Some(CopyKind::Manual));
        assert_eq!(
            kind_of("pre-migration-v2-to-v3-2026-11-02T120000-1.db"),
            Some(CopyKind::PreMigration)
        );
        assert_eq!(
            kind_of("pre-restore-2026-11-02T120000.db"),
            Some(CopyKind::PreRestore)
        );
        assert_eq!(kind_of("something-else.db"), None);
        assert_eq!(kind_of("lognotes-2026-11-02.txt"), None);
    }

    #[test]
    fn pruning_a_missing_directory_is_not_an_error() {
        prune(Path::new("/nonexistent/lognotes/backups"));
    }

    fn database(path: &Path, text: &str) -> Store {
        let mut store = Store::open(path).expect("a store");
        let day = store.ensure_journal_page("2026-11-02").expect("a day");
        store.insert_block(day, None, text).expect("a block");
        store
    }

    fn first_block(store: &Store) -> String {
        store.day("2026-11-02").expect("a day").blocks[0]
            .content
            .clone()
    }

    #[test]
    fn a_restore_swaps_in_the_chosen_graph_and_keeps_the_old_one() {
        let dir = temp_dir("lognotes-restore-success");
        let live = dir.join("lognotes.db");
        let chosen = dir.join("chosen.db");
        let backups = dir.join("backups");

        let current = database(&live, "current note");
        let chosen_store = database(&chosen, "restored note");
        drop(chosen_store);

        let prepared =
            restore_database(Some(&current), &live, &chosen, &backups).expect("a prepared restore");
        let pre_restore = prepared.pre_restore().to_path_buf();
        drop(current);

        let saved = prepared.apply().expect("the swap");
        assert_eq!(saved, pre_restore);

        let reopened = Store::open(&live).expect("the restored graph");
        assert_eq!(first_block(&reopened), "restored note");
        let old = Store::open(&pre_restore).expect("the pre-restore copy");
        assert_eq!(first_block(&old), "current note");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unusable_backup_is_refused_before_the_live_files_move() {
        let dir = temp_dir("lognotes-restore-invalid");
        let live = dir.join("lognotes.db");
        let chosen = dir.join("chosen.db");
        let backups = dir.join("backups");

        let current = database(&live, "current note");
        fs::write(&chosen, b"not a database").expect("the bad file");

        let result = restore_database(Some(&current), &live, &chosen, &backups);
        assert!(result.is_err());
        assert_eq!(first_block(&current), "current note");
        assert!(
            !live.with_file_name("lognotes.db.restoring").exists(),
            "the staged copy was cleaned up"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unopenable_live_file_is_set_aside_before_the_restore() {
        let dir = temp_dir("lognotes-restore-unopenable");
        let live = dir.join("lognotes.db");
        let chosen = dir.join("chosen.db");
        let backups = dir.join("backups");

        fs::write(&live, b"not a database").expect("the broken live file");
        let chosen_store = database(&chosen, "restored note");
        drop(chosen_store);

        let prepared = restore_database(None, &live, &chosen, &backups).expect("a prepared restore");
        let saved = prepared.apply().expect("the swap");

        let reopened = Store::open(&live).expect("the restored graph");
        assert_eq!(first_block(&reopened), "restored note");
        assert!(saved.exists(), "the unopenable file was kept aside");
        assert!(Store::open(&saved).is_err(), "it is still the unopenable file");

        let _ = fs::remove_dir_all(&dir);
    }
}
