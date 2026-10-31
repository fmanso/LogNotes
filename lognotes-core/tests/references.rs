//! Links, tags and references: the derived rows must follow the text, never lead it.

use lognotes_core::{Marker, ReferenceKind, Store};

fn store() -> Store {
    Store::open_in_memory().expect("in-memory store")
}

fn write(store: &mut Store, day: &str, content: &str) -> (i64, i64) {
    let page = store.ensure_journal_page(day).unwrap();
    let block = store.insert_block(page, None, content).unwrap();
    store.update_content(block, content).unwrap();
    (page, block)
}

#[test]
fn writing_a_link_creates_the_page_it_names() {
    let mut store = store();
    write(&mut store, "2026-04-13", "see [[My Project]]");

    let page = store.page_by_name("My Project").unwrap().expect("page created");
    assert_eq!(page.name, "My Project");
    assert_eq!(page.kind, lognotes_core::PageKind::Page);
}

#[test]
fn tags_create_pages_too() {
    let mut store = store();
    write(&mut store, "2026-04-13", "an #idea worth keeping");

    assert!(store.page_by_name("idea").unwrap().is_some());
}

#[test]
fn a_link_shows_up_as_a_linked_reference() {
    let mut store = store();
    let (_, block) = write(&mut store, "2026-04-13", "see [[My Project]]");
    let target = store.page_by_name("My Project").unwrap().unwrap();

    let references = store.references(target.id).unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].block.id, block);
    assert_eq!(references[0].kind, ReferenceKind::Linked);
    assert_eq!(references[0].journal_date.as_deref(), Some("2026-04-13"));
}

#[test]
fn removing_the_link_removes_the_reference() {
    let mut store = store();
    let (_, block) = write(&mut store, "2026-04-13", "see [[My Project]]");
    let target = store.page_by_name("My Project").unwrap().unwrap();

    store.update_content(block, "no links here").unwrap();
    assert!(store.references(target.id).unwrap().is_empty());
}

#[test]
fn a_plain_mention_alone_creates_no_page() {
    let mut store = store();
    write(&mut store, "2026-04-12", "talked about My Project today");

    // Mentions only ever match pages that exist; they do not conjure one from prose.
    assert!(store.page_by_name("My Project").unwrap().is_none());
}

#[test]
fn plain_mentions_of_an_existing_page_appear_once() {
    let mut store = store();
    let (_, mention) = write(&mut store, "2026-04-12", "talked about My Project today");
    let (_, linked) = write(&mut store, "2026-04-13", "see [[My Project]]");
    let target = store.page_by_name("My Project").unwrap().unwrap();

    let references = store.references(target.id).unwrap();
    let ids: Vec<i64> = references.iter().map(|row| row.block.id).collect();

    assert!(ids.contains(&linked));
    assert!(ids.contains(&mention));
    assert_eq!(references.iter().filter(|row| row.block.id == linked).count(), 1, "a linked block is not also a mention");
}

#[test]
fn mentions_match_whole_words() {
    let mut store = store();
    write(&mut store, "2026-04-13", "see [[Pla]]");
    let target = store.page_by_name("Pla").unwrap().unwrap();
    write(&mut store, "2026-04-12", "Plastic is not a mention");

    let references = store.references(target.id).unwrap();
    assert_eq!(references.len(), 1, "only the explicit link");
}

#[test]
fn deleting_the_block_removes_its_references() {
    let mut store = store();
    let (_, block) = write(&mut store, "2026-04-13", "see [[My Project]]");
    let target = store.page_by_name("My Project").unwrap().unwrap();

    store.delete_block(block).unwrap();
    assert!(store.references(target.id).unwrap().is_empty());
}

#[test]
fn named_pages_are_listed_for_the_switcher_but_journals_are_not() {
    let mut store = store();
    store.ensure_journal_page("2026-04-13").unwrap();
    write(&mut store, "2026-04-13", "see [[My Project]] and #idea");

    let names: Vec<String> = store.pages().unwrap().into_iter().map(|page| page.name).collect();
    assert_eq!(names, ["idea", "My Project"]);
}

#[test]
fn scheduling_and_markers_survive_a_reindex() {
    let mut store = store();
    let (_, block) = write(&mut store, "2026-04-13", "see [[My Project]]");
    store.set_scheduled(block, Some("2026-04-14")).unwrap();

    let page = store.journal_page("2026-04-13").unwrap().unwrap();
    let row = &store.page_blocks(page.id).unwrap()[0];
    assert_eq!(row.marker, Some(Marker::Todo));
    assert_eq!(store.references(store.page_by_name("My Project").unwrap().unwrap().id).unwrap().len(), 1);
}
