//! Folding is view state that survives a snapshot and a restart, and never enters undo.

use lognotes_core::{Error, Store};

#[test]
fn folding_hides_children_and_reports_the_child_count() {
    let mut store = Store::open_in_memory().expect("a store");
    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    let parent = store.insert_block(day, None, "parent").expect("parent");
    let child = store
        .insert_block(day, Some(parent), "child")
        .expect("child");
    store.indent(child).expect("nest the child");
    store
        .insert_block(day, Some(child), "grandchild")
        .expect("grandchild");
    let grandchild = store
        .page_blocks(day)
        .expect("blocks")
        .into_iter()
        .find(|row| row.content == "grandchild")
        .expect("the grandchild");
    store.indent(grandchild.id).expect("nest the grandchild");

    let rows = store.page_blocks(day).expect("blocks");
    let root = rows
        .iter()
        .find(|row| row.id == parent)
        .expect("the parent");
    assert_eq!(root.child_count, 1);
    assert!(!root.collapsed);

    store.set_collapsed(parent, true).expect("fold");
    let rows = store.page_blocks(day).expect("blocks");
    assert_eq!(rows.len(), 1, "only the folded parent is visible");
    assert!(rows[0].collapsed);
    assert_eq!(rows[0].child_count, 1);

    store.set_collapsed(parent, false).expect("unfold");
    assert_eq!(store.page_blocks(day).expect("blocks").len(), 3);
}

#[test]
fn folding_survives_a_reopened_store() {
    let path = std::env::temp_dir().join(format!("lognotes-fold-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let page;
    {
        let mut store = Store::open(&path).expect("a store");
        page = store.ensure_journal_page("2026-11-02").expect("a day");
        let parent = store.insert_block(page, None, "parent").expect("parent");
        let child = store
            .insert_block(page, Some(parent), "child")
            .expect("child");
        store.indent(child).expect("nest");
        store.set_collapsed(parent, true).expect("fold");
    }

    let store = Store::open(&path).expect("the reopened store");
    let rows = store.page_blocks(page).expect("blocks");
    assert_eq!(rows.len(), 1);
    assert!(rows[0].collapsed);

    drop(store);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn folding_is_not_an_undo_step() {
    let mut store = Store::open_in_memory().expect("a store");
    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    let parent = store.insert_block(day, None, "parent").expect("parent");
    let child = store
        .insert_block(day, Some(parent), "child")
        .expect("child");
    store.indent(child).expect("nest");

    store.set_collapsed(parent, true).expect("fold");
    assert!(store.undo().expect("undo"), "the last edit is still on top");
    let rows = store.page_blocks(day).expect("blocks");
    assert_eq!(rows.len(), 2, "the undo reverted the indent, not the fold");
    assert!(
        rows.iter()
            .find(|row| row.id == parent)
            .expect("the parent")
            .collapsed,
        "the fold was not undone"
    );

    assert!(matches!(
        store.set_collapsed(999_999, true),
        Err(Error::NotFound(_))
    ));
}
