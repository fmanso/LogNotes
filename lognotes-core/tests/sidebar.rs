//! The sidebar's two lists and the app's own settings: starred pages, recently opened pages, and the theme.

use lognotes_core::Store;

fn store() -> Store {
    Store::open_in_memory().expect("in-memory store")
}

#[test]
fn a_page_can_be_starred_and_unstarred() {
    let mut store = store();
    let page = store.ensure_page("My Project").unwrap();

    assert!(!store.is_favourite(page).unwrap());
    store.set_favourite(page, true).unwrap();
    assert!(store.is_favourite(page).unwrap());
    store.set_favourite(page, false).unwrap();
    assert!(!store.is_favourite(page).unwrap());
}

#[test]
fn favourites_list_by_name_and_exclude_journals() {
    let mut store = store();
    for name in ["Reading list", "My Project", "Boiler"] {
        let page = store.ensure_page(name).unwrap();
        store.set_favourite(page, true).unwrap();
    }
    store.ensure_journal_page("2026-04-13").unwrap();

    let favourites: Vec<String> = store
        .pages_by_favourite()
        .unwrap()
        .into_iter()
        .map(|page| page.name)
        .collect();

    assert_eq!(favourites, ["Boiler", "My Project", "Reading list"]);
}

#[test]
fn recents_list_the_newest_opening_first() {
    let mut store = store();
    let first = store.ensure_page("First").unwrap();
    let second = store.ensure_page("Second").unwrap();

    store.record_visit(first).unwrap();
    store.record_visit(second).unwrap();

    let recent: Vec<String> = store
        .recent_pages(8)
        .unwrap()
        .into_iter()
        .map(|page| page.name)
        .collect();
    assert_eq!(recent, ["Second", "First"]);
}

#[test]
fn opening_a_page_again_moves_it_back_to_the_top() {
    let mut store = store();
    let first = store.ensure_page("First").unwrap();
    let second = store.ensure_page("Second").unwrap();
    store.record_visit(first).unwrap();
    store.record_visit(second).unwrap();
    store.record_visit(first).unwrap();

    let recent: Vec<String> = store
        .recent_pages(8)
        .unwrap()
        .into_iter()
        .map(|page| page.name)
        .collect();
    assert_eq!(recent, ["First", "Second"]);
}

#[test]
fn recents_are_capped_and_skip_pages_never_opened() {
    let mut store = store();
    for index in 0..4 {
        let page = store.ensure_page(&format!("Page {index}")).unwrap();
        store.record_visit(page).unwrap();
    }
    store.ensure_page("Never opened").unwrap();

    let recent = store.recent_pages(2).unwrap();

    assert_eq!(recent.len(), 2);
    assert!(recent.iter().all(|page| page.name.starts_with("Page")));
}

#[test]
fn a_journal_day_is_not_a_recent_page() {
    let mut store = store();
    let day = store.ensure_journal_page("2026-04-13").unwrap();
    store.record_visit(day).unwrap();

    assert!(store.recent_pages(8).unwrap().is_empty());
}

#[test]
fn a_setting_survives_being_written_and_read() {
    let mut store = store();

    assert_eq!(store.setting("theme").unwrap(), None);
    store.set_setting("theme", "dark").unwrap();
    assert_eq!(store.setting("theme").unwrap().as_deref(), Some("dark"));
    store.set_setting("theme", "light").unwrap();
    assert_eq!(store.setting("theme").unwrap().as_deref(), Some("light"));
}

#[test]
fn starring_and_opening_do_not_enter_the_undo_history() {
    let mut store = store();
    let page = store.ensure_page("My Project").unwrap();
    store.set_favourite(page, true).unwrap();
    store.record_visit(page).unwrap();

    assert!(!store.can_undo(), "a star is not an edit");
}
