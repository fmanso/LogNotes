//! Behaviour the journal outliner depends on: ordering, structural edits, and an undo history that
//! matches what the user did rather than what the code did.

use lognotes_core::{Marker, PageKind, Store};

fn store() -> Store {
    Store::open_in_memory().expect("in-memory store")
}

fn contents(store: &Store, page: i64) -> Vec<String> {
    store
        .page_blocks(page)
        .unwrap()
        .into_iter()
        .map(|block| block.content)
        .collect()
}

fn depths(store: &Store, page: i64) -> Vec<u32> {
    store
        .page_blocks(page)
        .unwrap()
        .into_iter()
        .map(|block| block.depth)
        .collect()
}

#[test]
fn blocks_keep_typed_order_and_nesting() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();

    let one = store.insert_block(page, None, "one").unwrap();
    let two = store.insert_block(page, None, "two").unwrap();
    // `after` means "next sibling of", so nesting is what indent does.
    let child = store.insert_block(page, Some(one), "child").unwrap();
    let grandchild = store.insert_block(page, Some(child), "grandchild").unwrap();
    assert_eq!(contents(&store, page), ["one", "child", "grandchild", "two"]);
    assert_eq!(depths(&store, page), [0, 0, 0, 0]);

    // Nest from the bottom up: after `child` moves away, `grandchild`'s previous sibling is `one`.
    store.indent(grandchild).unwrap();
    store.indent(child).unwrap();

    assert_eq!(contents(&store, page), ["one", "child", "grandchild", "two"]);
    assert_eq!(depths(&store, page), [0, 1, 2, 0]);

    let blocks = store.page_blocks(page).unwrap();
    assert_eq!(blocks[1].parent_id, Some(one));
    assert_eq!(blocks[2].parent_id, Some(child));
    assert_eq!(blocks[3].parent_id, None);
    assert_eq!(blocks[3].id, two);
}

#[test]
fn indent_nests_under_the_previous_sibling_and_outdent_restores() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();
    let a = store.insert_block(page, None, "a").unwrap();
    let b = store.insert_block(page, Some(a), "b").unwrap();

    store.indent(b).unwrap();
    assert_eq!(depths(&store, page), [0, 1]);
    assert_eq!(store.page_blocks(page).unwrap()[1].parent_id, Some(a));

    store.outdent(b).unwrap();
    assert_eq!(depths(&store, page), [0, 0]);
}

#[test]
fn indenting_a_first_sibling_records_no_history() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();
    let only = store.insert_block(page, None, "only").unwrap();

    // There is no previous sibling, so indent is a no-op — and a no-op must not enter the undo trail.
    store.indent(only).unwrap();
    assert_eq!(depths(&store, page), [0]);

    assert!(store.undo().unwrap());
    assert!(
        store.page_blocks(page).unwrap().is_empty(),
        "undoing must remove the block, not unwind a phantom indent"
    );
}

#[test]
fn content_edits_undo_and_redo_symmetrically() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();
    let block = store.insert_block(page, None, "hello").unwrap();

    store.update_content(block, "hello world").unwrap();
    assert_eq!(contents(&store, page), ["hello world"]);

    assert!(store.undo().unwrap());
    assert_eq!(contents(&store, page), ["hello"]);

    assert!(store.redo().unwrap());
    assert_eq!(contents(&store, page), ["hello world"]);
}

#[test]
fn deleting_a_subtree_is_one_undo_step() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();
    let parent = store.insert_block(page, None, "parent").unwrap();
    let child = store.insert_block(page, Some(parent), "child").unwrap();
    let grandchild = store.insert_block(page, Some(child), "grandchild").unwrap();
    store.insert_block(page, None, "sibling").unwrap();
    store.indent(grandchild).unwrap();
    store.indent(child).unwrap();
    assert_eq!(depths(&store, page), [0, 1, 2, 0]);

    store.delete_block(parent).unwrap();
    assert_eq!(contents(&store, page), ["sibling"]);

    assert!(store.undo().unwrap());
    assert_eq!(contents(&store, page), ["parent", "child", "grandchild", "sibling"]);
    assert_eq!(depths(&store, page), [0, 1, 2, 0]);
    assert!(store.can_undo(), "only the delete was undone, the indents remain");
}

#[test]
fn split_is_a_single_undo_step() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();
    let block = store.insert_block(page, None, "helloworld").unwrap();

    let tail = store.split_block(block, 5).unwrap();
    assert_eq!(contents(&store, page), ["hello", "world"]);

    assert!(store.undo().unwrap());
    assert_eq!(contents(&store, page), ["helloworld"], "the split undid as one step");

    assert!(store.redo().unwrap());
    assert_eq!(contents(&store, page), ["hello", "world"]);
    assert_eq!(store.page_blocks(page).unwrap()[1].id, tail);
}

#[test]
fn split_refuses_a_mid_character_offset() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();
    let block = store.insert_block(page, None, "héllo").unwrap();

    // "é" is two bytes; offset 2 lands inside it.
    assert!(store.split_block(block, 2).is_err());
    assert_eq!(contents(&store, page), ["héllo"]);
}

#[test]
fn undo_crosses_days_in_order() {
    let mut store = store();
    let day_one = store.ensure_journal_page("2026-04-12").unwrap();
    let day_two = store.ensure_journal_page("2026-04-13").unwrap();
    store.insert_block(day_one, None, "yesterday").unwrap();
    store.insert_block(day_two, None, "today").unwrap();

    assert!(store.undo().unwrap());
    assert_eq!(contents(&store, day_two), Vec::<String>::new());
    assert_eq!(contents(&store, day_one), ["yesterday"]);

    assert!(store.undo().unwrap());
    assert_eq!(contents(&store, day_one), Vec::<String>::new());
    assert!(!store.can_undo());
}

#[test]
fn new_edits_clear_the_redo_trail() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();
    store.insert_block(page, None, "one").unwrap();
    store.undo().unwrap();

    assert!(store.can_redo());
    store.insert_block(page, None, "two").unwrap();
    assert!(!store.can_redo());
}

#[test]
fn reading_a_day_does_not_create_it() {
    let store = store();
    let day = store.day("2026-04-13").unwrap();

    assert_eq!(day.page_id, None);
    assert!(day.blocks.is_empty());
    assert!(store.journal_page("2026-04-13").unwrap().is_none());
}

#[test]
fn journal_pages_are_unique_per_date() {
    let mut store = store();
    let first = store.ensure_journal_page("2026-04-13").unwrap();
    let second = store.ensure_journal_page("2026-04-13").unwrap();

    assert_eq!(first, second);
    let page = store.page(first).unwrap();
    assert_eq!(page.kind, PageKind::Journal);
    assert_eq!(page.journal_date.as_deref(), Some("2026-04-13"));
}

#[test]
fn page_names_are_matched_case_insensitively() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();
    let block = store.insert_block(page, None, "x").unwrap();
    store.update_content(block, "x").unwrap();

    assert!(store.page_by_name("2026-04-13").unwrap().is_some());
    assert!(store.page_by_name("  nope ").unwrap().is_none());
    assert!(store.page(page).unwrap().name_key.eq("2026-04-13"));
}

#[test]
fn day_chunks_run_backwards_from_a_date() {
    let store = store();
    let days = store.journal_days_ending_at("2026-04-13", 3).unwrap();

    assert_eq!(days, ["2026-04-13", "2026-04-12", "2026-04-11"]);
}

#[test]
fn malformed_dates_are_rejected() {
    let store = store();
    assert!(store.day("13/04/2026").is_err());
    assert!(store.journal_page("2026-4-3").is_err());
}

#[test]
fn markers_and_dates_are_absent_in_this_build() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();
    store.insert_block(page, None, "plain").unwrap();

    let block = &store.page_blocks(page).unwrap()[0];
    assert_eq!(block.marker, None);
    assert_eq!(block.scheduled_date, None);
    assert_eq!(block.marker, Option::<Marker>::None);
}
