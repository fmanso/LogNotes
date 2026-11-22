//! Upgrading a graph that was written by an older build.
//!
//! The fixtures are the schemas as they shipped, with a small graph each. The live schema is built by
//! the store; these tests are the only ones that start from something older.

use std::path::{Path, PathBuf};

use lognotes_core::{Error, Marker, PageKind, ReferenceKind, Store};
use rusqlite::Connection;

fn temp(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("lognotes-migrations-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a directory");
    path
}

fn fixture(name: &str) -> &'static str {
    match name {
        "v1" => include_str!("fixtures/v1.sql"),
        "v2" => include_str!("fixtures/v2.sql"),
        other => panic!("unknown fixture {other}"),
    }
}

/// Write a fixture into a fresh database file and return it.
fn fixture_database(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join("lognotes.db");
    let conn = Connection::open(&path).expect("a connection");
    conn.execute_batch(fixture(name)).expect("the fixture");
    conn.close().expect("close");
    path
}

fn version(path: &Path) -> i64 {
    let conn = Connection::open(path).expect("a connection");
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("a version")
}

fn page_has_column(path: &Path, column: &str) -> bool {
    let conn = Connection::open(path).expect("a connection");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('page') WHERE name = ?1",
            [column],
            |row| row.get(0),
        )
        .expect("table info");
    count > 0
}

#[test]
fn fixtures_open_at_the_version_they_declare() {
    for (name, expected) in [("v1", 1), ("v2", 2)] {
        let dir = temp(&format!("fixture-{name}"));
        let path = fixture_database(&dir, name);
        assert_eq!(version(&path), expected, "{name} declares its version");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn older_graphs_reach_the_current_schema_with_their_data() {
    for name in ["v1", "v2"] {
        let dir = temp(&format!("upgrade-{name}"));
        let path = fixture_database(&dir, name);

        let store = Store::open(&path).expect("an upgrade");
        assert_eq!(version(&path), 3, "{name} reaches the current version");

        let project = store
            .page_by_name("My Project")
            .expect("a lookup")
            .expect("the page");
        assert_eq!(project.kind, PageKind::Page);
        let blocks = store.page_blocks(project.id).expect("blocks");
        assert_eq!(blocks[0].content, "the project home");

        let day = store.day("2026-11-02").expect("the day");
        assert_eq!(day.blocks.len(), 1);
        assert_eq!(day.blocks[0].marker, Some(Marker::Todo));
        assert_eq!(day.blocks[0].scheduled_date.as_deref(), Some("2026-11-05"));

        // The link table and the FTS index both survived (or were rebuilt) and agree with the text.
        let references = store.references(project.id).expect("references");
        assert!(
            references
                .iter()
                .any(|row| row.kind == ReferenceKind::Linked
                    && row.block.content.contains("working on")),
            "{name} kept its link: {references:?}"
        );
        assert!(store.search("working", 0, 10).expect("search").total > 0);

        // The v3 additions are there: a setting can be written, and the page can be starred.
        store.setting("theme").expect("the setting table");
        assert!(
            !store
                .is_favourite(project.id)
                .expect("the favourite column")
        );

        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_newer_schema_is_refused_and_left_alone() {
    let dir = temp("newer");
    let path = fixture_database(&dir, "v2");
    {
        let conn = Connection::open(&path).expect("a connection");
        conn.pragma_update(None, "user_version", 99)
            .expect("a version");
    }

    match Store::open(&path) {
        Err(Error::SchemaTooNew {
            found: 99,
            supported: 3,
        }) => {}
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("a newer schema was opened"),
    }
    assert_eq!(version(&path), 99, "the file was not touched");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_step_rolls_back_and_the_next_open_retries() {
    let dir = temp("rollback");
    let path = fixture_database(&dir, "v2");
    {
        // The v3 step creates a `setting` table. A legacy one makes that statement fail after the
        // two ALTERs have run; the transaction must take the ALTERs back with it.
        let conn = Connection::open(&path).expect("a connection");
        conn.execute_batch("CREATE TABLE setting (legacy TEXT);")
            .expect("the conflict");
    }

    assert!(Store::open(&path).is_err(), "the v3 step must fail");
    assert_eq!(version(&path), 2, "the version did not advance");
    assert!(
        !page_has_column(&path, "favourite"),
        "the failed ALTERs rolled back"
    );

    {
        let conn = Connection::open(&path).expect("a connection");
        conn.execute_batch("DROP TABLE setting;").expect("drop");
    }

    let store = Store::open(&path).expect("the retry");
    assert_eq!(version(&path), 3);
    let project = store
        .page_by_name("My Project")
        .expect("a lookup")
        .expect("the page");
    assert!(!store.is_favourite(project.id).expect("the column"));

    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

fn pre_migration_copies(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("pre-migration-") && name.ends_with(".db"))
        })
        .collect()
}

#[test]
fn an_upgrade_writes_the_old_graph_before_it_changes_it() {
    let dir = temp("pre-migration");
    let path = fixture_database(&dir, "v2");
    let backups = dir.join("backups");

    let store = Store::open_with_migration_backup(&path, &backups).expect("an upgrade");
    assert_eq!(version(&path), 3);

    let copies = pre_migration_copies(&backups);
    assert_eq!(copies.len(), 1, "one copy: {copies:?}");
    assert_eq!(version(&copies[0]), 2, "the copy is the old version");
    assert_eq!(
        store.migration_copy(),
        Some(copies[0].as_path()),
        "the store knows which copy it made"
    );

    let copy = Store::open(&copies[0]).expect("the copy opens");
    let day = copy.day("2026-11-02").expect("the day");
    assert_eq!(day.blocks[0].content, "working on [[My Project]] #journal");

    drop(copy);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unusable_backup_directory_blocks_the_upgrade() {
    let dir = temp("pre-migration-failure");
    let path = fixture_database(&dir, "v2");
    let blocker = dir.join("not-a-directory");
    std::fs::write(&blocker, b"x").expect("the blocker");
    let backups = blocker.join("backups");

    assert!(
        Store::open_with_migration_backup(&path, &backups).is_err(),
        "an upgrade without a copy must not run"
    );
    assert_eq!(version(&path), 2, "the live graph did not move");
    assert!(!page_has_column(&path, "favourite"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_fresh_database_needs_no_pre_migration_copy() {
    let dir = temp("pre-migration-fresh");
    let path = dir.join("lognotes.db");
    let backups = dir.join("backups");

    let store = Store::open_with_migration_backup(&path, &backups).expect("a fresh store");
    assert_eq!(version(&path), 3);
    assert!(
        pre_migration_copies(&backups).is_empty(),
        "a new graph has nothing to copy"
    );

    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}
