//! The quick switcher's query: pages by name first, then blocks by their text.

use lognotes_core::Store;

fn store() -> Store {
    Store::open_in_memory().expect("in-memory store")
}

fn write(store: &mut Store, day: &str, content: &str) -> i64 {
    let page = store.ensure_journal_page(day).unwrap();
    let block = store.insert_block(page, None, content).unwrap();
    store.update_content(block, content).unwrap();
    block
}

#[test]
fn pages_rank_exact_then_prefix_then_anywhere() {
    let mut store = store();
    store.ensure_page("project").unwrap();
    store.ensure_page("project notes").unwrap();
    store.ensure_page("My Project").unwrap();
    store.ensure_page("unrelated").unwrap();

    let found = store.search("project", 10, 10).unwrap();
    let names: Vec<&str> = found.pages.iter().map(|page| page.name.as_str()).collect();

    // Exact first, then the name that starts with it, then the one that merely contains it.
    assert_eq!(names, ["project", "project notes", "My Project"]);
}

#[test]
fn a_page_search_is_capped_and_excludes_journals() {
    let mut store = store();
    for index in 0..6 {
        store.ensure_page(&format!("note {index}")).unwrap();
    }
    store.ensure_journal_page("2026-04-13").unwrap();

    let found = store.search("note", 4, 10).unwrap();

    assert_eq!(found.pages.len(), 4, "the list is capped, not the index");
    assert!(found.pages.iter().all(|page| page.name.starts_with("note")));
}

#[test]
fn a_half_typed_word_already_matches_block_text() {
    let mut store = store();
    let block = write(&mut store, "2026-04-13", "call the plumber about the boiler");

    let found = store.search("plumb", 5, 20).unwrap();

    assert_eq!(found.blocks.len(), 1);
    assert_eq!(found.blocks[0].block.id, block);
    assert_eq!(found.blocks[0].journal_date.as_deref(), Some("2026-04-13"));
    assert!(found.blocks[0].snippet.contains("plumber"));
}

#[test]
fn words_inside_a_block_match_too() {
    let mut store = store();
    write(&mut store, "2026-04-13", "the boiler needs a service");

    assert_eq!(store.search("boiler", 5, 20).unwrap().blocks.len(), 1);
    assert_eq!(store.search("service", 5, 20).unwrap().blocks.len(), 1);
}

#[test]
fn a_block_hit_says_which_page_it_lives_on() {
    let mut store = store();
    let page = store.ensure_page("Reading list").unwrap();
    store.insert_block(page, None, "finish the search chapter").unwrap();

    let found = store.search("chapter", 5, 20).unwrap();

    assert_eq!(found.blocks.len(), 1);
    assert_eq!(found.blocks[0].page_name, "Reading list");
    assert_eq!(found.blocks[0].journal_date, None);
}

#[test]
fn nothing_matching_is_an_empty_answer_not_an_error() {
    let mut store = store();
    write(&mut store, "2026-04-13", "hello");

    let found = store.search("zeppelin", 5, 20).unwrap();

    assert!(found.pages.is_empty());
    assert!(found.blocks.is_empty());
}

#[test]
fn an_empty_query_finds_nothing() {
    let mut store = store();
    write(&mut store, "2026-04-13", "hello");

    let found = store.search("   ", 5, 20).unwrap();

    assert!(found.pages.is_empty() && found.blocks.is_empty());
}

#[test]
fn a_long_block_is_shown_as_a_window_around_the_match() {
    let mut store = store();
    let long = format!("{}needle{}", "word ".repeat(40), " tail".repeat(40));
    write(&mut store, "2026-04-13", &long);

    let found = store.search("needle", 5, 20).unwrap();
    let snippet = &found.blocks[0].snippet;

    assert!(snippet.contains("needle"));
    assert!(snippet.starts_with('…') && snippet.ends_with('…'), "got {snippet:?}");
    assert!(snippet.chars().count() < long.chars().count() / 2);
}

#[test]
fn a_block_remembers_the_page_it_belongs_to() {
    let mut store = store();
    let page = store.ensure_page("My Project").unwrap();
    let block = store.insert_block(page, None, "a note").unwrap();

    assert_eq!(store.page_of_block(block).unwrap().id, page);
}

#[test]
fn query_punctuation_cannot_break_the_search() {
    let mut store = store();
    write(&mut store, "2026-04-13", "a note about the \"boiler\" and (pipes)");

    // None of these is FTS syntax once the query is quoted; all of them must answer, not error.
    for query in ["\"boiler", "pipes)", "a-note", "*", "(pipes"] {
        assert!(store.search(query, 5, 20).is_ok(), "query {query:?} was refused");
    }
}
