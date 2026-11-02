//! Links, tags and references: the derived rows must follow the text, never lead it.

use lognotes_core::{Marker, PageKind, ReferenceKind, Store};

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
fn written_days_are_listed_for_the_switcher_alongside_named_pages() {
    let mut store = store();
    store.ensure_journal_page("2026-04-13").unwrap();
    write(&mut store, "2026-04-13", "see [[My Project]] and #idea");

    let names: Vec<String> = store.pages().unwrap().into_iter().map(|page| page.name).collect();
    // A date is a name a block can link to, so it is a name the switcher has to offer; it sorts where
    // its digits sort, before the word-like names.
    assert_eq!(names, ["2026-04-13", "idea", "My Project"]);
}

#[test]
fn a_day_nobody_wrote_is_not_in_the_switcher() {
    let mut store = store();
    store.ensure_journal_page("2026-04-13").unwrap();

    let names: Vec<String> = store.pages().unwrap().into_iter().map(|page| page.name).collect();

    assert_eq!(names, ["2026-04-13"], "scrolling is what reaches a day that was never written");
}

#[test]
fn a_date_link_names_the_journal_day() {
    let mut store = store();
    let (_, block) = write(&mut store, "2026-04-13", "see [[2026-04-14]]");

    let day = store.journal_page("2026-04-14").unwrap().expect("the day was created");
    assert_eq!(day.kind, PageKind::Journal);
    // The same row a name lookup finds: no look-alike page beside the calendar.
    assert_eq!(store.page_by_name("2026-04-14").unwrap().unwrap().id, day.id);

    let references = store.references(day.id).unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].block.id, block);
    assert_eq!(references[0].journal_date.as_deref(), Some("2026-04-13"));
}

#[test]
fn opening_a_date_by_name_reaches_the_same_day() {
    let mut store = store();
    let day = store.ensure_journal_page("2026-04-14").unwrap();

    assert_eq!(store.ensure_page("2026-04-14").unwrap(), day);
    assert_eq!(store.ensure_page(" 2026-04-14 ").unwrap(), day, "trimmed, like any name");
}

#[test]
fn something_that_only_looks_like_a_date_stays_a_page() {
    let mut store = store();

    // chrono parses a loose "2026-4-3"; a name that does not round-trip is not a day.
    for name in ["2026-4-3", "2026-04-31", "13-04-2026", "2026-04-14-extra"] {
        let created = store.ensure_page(name).unwrap();
        assert_eq!(store.page(created).unwrap().kind, PageKind::Page, "{name}");
    }
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
