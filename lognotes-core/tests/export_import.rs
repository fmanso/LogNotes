//! The point of an export is that the notes survive it: what goes out has to come back.

use std::path::PathBuf;

use lognotes_core::{Marker, Store, export_markdown, import_markdown};

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a directory");
    path
}

#[test]
fn a_graph_survives_a_round_trip() {
    let dir = temp("lognotes-round-trip");
    let mut store = Store::open_in_memory().expect("a store");

    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    let first = store.insert_block(day, None, "hello #journal").expect("a block");
    store.set_marker(first, Some(Marker::Todo)).expect("a marker");
    store.set_scheduled(first, Some("2026-11-05")).expect("a date");
    // `after` inserts a sibling; indent it to make it a child.
    let child = store.insert_block(day, Some(first), "a child [[My Project]]").expect("a block");
    store.indent(child).expect("a child");
    store.update_content(child, "a child [[My Project]]").expect("indexed");

    let page = store.ensure_page("My Project").expect("a page");
    store.insert_block(page, None, "```python\ndef f():\n    return 1\n```").expect("a block");

    // Two pages, not one: `#journal` is a tag, and a tag is a page in its own right.
    let moved = export_markdown(&store, &dir).expect("an export");
    assert_eq!((moved.days, moved.pages, moved.blocks), (1, 2, 3));

    let mut fresh = Store::open_in_memory().expect("a fresh store");
    let read = import_markdown(&mut fresh, &dir).expect("an import");
    assert_eq!((read.days, read.pages, read.blocks), (1, 2, 3));

    // The day: the marker, the schedule, the nesting and the link all came back.
    let day_back = fresh.day("2026-11-02").expect("a day");
    assert_eq!(day_back.blocks.len(), 2);
    assert_eq!(day_back.blocks[0].content, "hello #journal");
    assert_eq!(day_back.blocks[0].marker, Some(Marker::Todo));
    assert_eq!(day_back.blocks[0].scheduled_date.as_deref(), Some("2026-11-05"));
    assert_eq!(day_back.blocks[1].content, "a child [[My Project]]");
    assert_eq!(day_back.blocks[1].depth, 1, "the child is still nested");

    // The page: one block, its three lines intact.
    let page_back = fresh.page_by_name("My Project").expect("a lookup").expect("a page");
    let blocks = fresh.page_blocks(page_back.id).expect("blocks");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].content, "```python\ndef f():\n    return 1\n```");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_export_writes_the_folders_logseq_reads() {
    let dir = temp("lognotes-export-shape");
    let mut store = Store::open_in_memory().expect("a store");
    store.ensure_journal_page("2026-11-02").expect("a day");
    store.ensure_page("My Project").expect("a page");

    export_markdown(&store, &dir).expect("an export");

    assert!(dir.join("journals/2026_11_02.md").is_file());
    assert!(dir.join("pages/My Project.md").is_file());

    let _ = std::fs::remove_dir_all(&dir);
}
