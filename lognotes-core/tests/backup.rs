//! A backup has to be a database: complete, consistent, and readable on its own.

use std::path::PathBuf;

use lognotes_core::Store;

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn a_backup_holds_what_the_database_held() {
    let live = temp("lognotes-backup-live.db");
    let copy = temp("lognotes-backup-copy.db");
    let mut store = Store::open(&live).expect("a store");
    let page = store.ensure_journal_page("2026-04-13").expect("a day");
    let block = store.insert_block(page, None, "keep this [[Page]]").expect("a block");
    store.update_content(block, "keep this [[Page]]").expect("indexed");

    store.backup_into(&copy).expect("a backup");

    // The copy opens on its own and has the note, the link and the full-text index.
    let backup = Store::open(&copy).expect("the backup opens");
    let day = backup.day("2026-04-13").expect("a day");
    assert_eq!(day.blocks.len(), 1);
    assert_eq!(day.blocks[0].content, "keep this [[Page]]");
    assert!(backup.page_by_name("Page").expect("a lookup").is_some());

    let _ = std::fs::remove_file(&live);
    let _ = std::fs::remove_file(&copy);
}

#[test]
fn backing_up_again_over_the_same_file_rewrites_it() {
    let live = temp("lognotes-backup-again.db");
    let copy = temp("lognotes-backup-again-copy.db");
    let mut store = Store::open(&live).expect("a store");
    store.ensure_page("First").expect("a page");
    store.backup_into(&copy).expect("a backup");
    store.ensure_page("Second").expect("another page");

    store.backup_into(&copy).expect("a second backup");

    let backup = Store::open(&copy).expect("the backup opens");
    assert!(backup.page_by_name("Second").expect("a lookup").is_some());
    assert!(backup.page_by_name("First").expect("a lookup").is_some());

    let _ = std::fs::remove_file(&live);
    let _ = std::fs::remove_file(&copy);
}
