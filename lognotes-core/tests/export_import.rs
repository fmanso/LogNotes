//! The point of an export is that the notes survive it: what goes out has to come back.

use std::path::PathBuf;

use lognotes_core::{ImportMode, Marker, Store, export_markdown, import_markdown};

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
    let read = import_markdown(&mut fresh, &dir, ImportMode::Merge).expect("an import");
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

fn folder(name: &str, pages: &[(&str, &str)]) -> PathBuf {
    let dir = temp(name);
    std::fs::create_dir_all(dir.join("pages")).expect("pages");
    for (page, body) in pages {
        std::fs::write(dir.join("pages").join(format!("{page}.md")), body).expect("a page");
    }
    dir
}

fn contents(store: &Store, name: &str) -> Vec<String> {
    store
        .page_by_name(name)
        .expect("a lookup")
        .map(|page| {
            store
                .page_blocks(page.id)
                .expect("blocks")
                .into_iter()
                .map(|block| block.content)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn merge_replaces_named_pages_and_leaves_the_rest_alone() {
    let dir = folder("lognotes-merge", &[("My Project", "- fresh project\n  - a child\n")]);
    let mut store = Store::open_in_memory().expect("a store");
    let other = store.ensure_page("Other").expect("a page");
    store.insert_block(other, None, "keep me").expect("a block");
    let project = store.ensure_page("My Project").expect("a page");
    store.insert_block(project, None, "old project").expect("a block");

    let moved = import_markdown(&mut store, &dir, ImportMode::Merge).expect("a merge");
    assert_eq!((moved.pages, moved.blocks), (1, 2));
    assert_eq!(contents(&store, "Other"), ["keep me"]);
    assert_eq!(contents(&store, "My Project"), ["fresh project", "a child"]);

    // The same folder again leaves the same graph.
    import_markdown(&mut store, &dir, ImportMode::Merge).expect("the second merge");
    assert_eq!(contents(&store, "Other"), ["keep me"]);
    assert_eq!(contents(&store, "My Project"), ["fresh project", "a child"]);
    assert_eq!(
        store.page_blocks(project).expect("blocks").len(),
        2,
        "a re-import did not duplicate the bullets"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn replace_all_clears_unnamed_pages_and_one_undo_restores_everything() {
    let dir = folder("lognotes-replace", &[("My Project", "- fresh project\n")]);
    let mut store = Store::open_in_memory().expect("a store");
    let other = store.ensure_page("Other").expect("a page");
    store.insert_block(other, None, "keep me").expect("a block");
    let project = store.ensure_page("My Project").expect("a page");
    store.insert_block(project, None, "old project").expect("a block");

    import_markdown(&mut store, &dir, ImportMode::ReplaceAll).expect("a replace");
    assert_eq!(contents(&store, "Other"), Vec::<String>::new());
    assert_eq!(contents(&store, "My Project"), ["fresh project"]);
    assert!(store.page_by_name("Other").expect("a lookup").is_some());

    store.undo().expect("one undo");
    assert_eq!(contents(&store, "Other"), ["keep me"]);
    assert_eq!(contents(&store, "My Project"), ["old project"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unreadable_page_stops_before_anything_is_written() {
    let dir = folder("lognotes-bad-page", &[("My Project", "- fresh project\n")]);
    std::fs::write(dir.join("pages").join("Broken.md"), [0xff, 0xfe, 0xfd]).expect("bad bytes");

    let mut store = Store::open_in_memory().expect("a store");
    let other = store.ensure_page("Other").expect("a page");
    store.insert_block(other, None, "keep me").expect("a block");

    let result = import_markdown(&mut store, &dir, ImportMode::Merge);
    let error = result.expect_err("the bad file stops the import").to_string();
    assert!(error.contains("Broken.md"), "the error names the file: {error}");
    assert_eq!(contents(&store, "Other"), ["keep me"]);
    assert!(
        store.page_by_name("My Project").expect("a lookup").is_none(),
        "nothing from the folder was created"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_page_name_that_looks_like_a_path_survives_the_round_trip() {
    let dir = temp("lognotes-escaped-name");
    let mut store = Store::open_in_memory().expect("a store");
    let name = "a/b:c\\d";
    let page = store.ensure_page(name).expect("a page");
    store.insert_block(page, None, "body").expect("a block");

    export_markdown(&store, &dir).expect("an export");
    let mut fresh = Store::open_in_memory().expect("a fresh store");
    import_markdown(&mut fresh, &dir, ImportMode::Merge).expect("an import");

    let back = fresh
        .page_by_name(name)
        .expect("a lookup")
        .expect("the page");
    assert_eq!(contents(&fresh, name), ["body"]);
    assert_eq!(back.name, name);

    let _ = std::fs::remove_dir_all(&dir);
}
