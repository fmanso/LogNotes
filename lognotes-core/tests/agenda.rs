//! The agenda rule from `lognotes`: TODO/DOING within a horizon, overdue kept, DONE gone.

use lognotes_core::{Marker, Store};

fn store() -> Store {
    Store::open_in_memory().expect("in-memory store")
}

fn scheduled(store: &mut Store, day: &str, content: &str) -> i64 {
    let page = store.ensure_journal_page(day).unwrap();
    store.insert_block(page, None, content).unwrap()
}

fn contents(store: &Store, today: &str) -> Vec<String> {
    store
        .agenda(today, 7)
        .unwrap()
        .into_iter()
        .map(|row| row.block.content)
        .collect()
}

#[test]
fn the_cycle_runs_none_todo_doing_done_none() {
    let mut store = store();
    let page = store.ensure_journal_page("2026-04-13").unwrap();
    let block = store.insert_block(page, None, "x").unwrap();

    let marker = |store: &Store| store.page_blocks(page).unwrap()[0].marker;

    assert_eq!(marker(&store), None);
    store.cycle_marker(block).unwrap();
    assert_eq!(marker(&store), Some(Marker::Todo));
    store.cycle_marker(block).unwrap();
    assert_eq!(marker(&store), Some(Marker::Doing));
    store.cycle_marker(block).unwrap();
    assert_eq!(marker(&store), Some(Marker::Done));
    store.cycle_marker(block).unwrap();
    assert_eq!(marker(&store), None);
}

#[test]
fn the_agenda_stops_at_the_horizon_and_keeps_the_boundaries() {
    let mut store = store();
    let overdue = scheduled(&mut store, "2026-04-10", "overdue");
    let today = scheduled(&mut store, "2026-04-13", "today");
    let edge = scheduled(&mut store, "2026-04-20", "day seven");
    let beyond = scheduled(&mut store, "2026-04-21", "day eight");

    for block in [overdue, today, edge, beyond] {
        store.set_scheduled(block, Some("2026-04-13")).unwrap();
    }
    // Give each its own date now that they are all tasks.
    store.set_scheduled(overdue, Some("2026-04-10")).unwrap();
    store.set_scheduled(today, Some("2026-04-13")).unwrap();
    store.set_scheduled(edge, Some("2026-04-20")).unwrap();
    store.set_scheduled(beyond, Some("2026-04-21")).unwrap();

    assert_eq!(contents(&store, "2026-04-13"), ["overdue", "today", "day seven"]);
}

#[test]
fn overdue_rows_are_flagged_and_sorted_first() {
    let mut store = store();
    let later = scheduled(&mut store, "2026-04-13", "later");
    let earlier = scheduled(&mut store, "2026-04-13", "earlier");
    store.set_scheduled(later, Some("2026-04-14")).unwrap();
    store.set_scheduled(earlier, Some("2026-04-11")).unwrap();

    let rows = store.agenda("2026-04-13", 7).unwrap();
    assert_eq!(rows[0].block.content, "earlier");
    assert!(rows[0].overdue);
    assert_eq!(rows[1].block.content, "later");
    assert!(!rows[1].overdue);
}

#[test]
fn done_blocks_leave_the_agenda() {
    let mut store = store();
    let block = scheduled(&mut store, "2026-04-13", "finish me");
    store.set_scheduled(block, Some("2026-04-13")).unwrap();
    assert_eq!(contents(&store, "2026-04-13"), ["finish me"]);

    store.set_marker(block, Some(Marker::Done)).unwrap();
    assert!(contents(&store, "2026-04-13").is_empty());
}

#[test]
fn scheduling_an_untasked_block_also_marks_it_todo() {
    let mut store = store();
    let block = scheduled(&mut store, "2026-04-13", "plain");
    assert_eq!(store.page_blocks(store.journal_page("2026-04-13").unwrap().unwrap().id).unwrap()[0].marker, None);

    store.set_scheduled(block, Some("2026-04-13")).unwrap();

    let row = &store.page_blocks(store.journal_page("2026-04-13").unwrap().unwrap().id).unwrap()[0];
    assert_eq!(row.marker, Some(Marker::Todo));
    assert_eq!(row.scheduled_date.as_deref(), Some("2026-04-13"));
    assert_eq!(contents(&store, "2026-04-13"), ["plain"]);
}

#[test]
fn one_undo_removes_both_the_date_and_the_auto_marker() {
    let mut store = store();
    let block = scheduled(&mut store, "2026-04-13", "plain");
    store.set_scheduled(block, Some("2026-04-13")).unwrap();

    assert!(store.undo().unwrap());

    let page = store.journal_page("2026-04-13").unwrap().unwrap();
    let row = &store.page_blocks(page.id).unwrap()[0];
    assert_eq!(row.marker, None);
    assert_eq!(row.scheduled_date, None);
}

#[test]
fn clearing_the_date_keeps_the_marker() {
    let mut store = store();
    let block = scheduled(&mut store, "2026-04-13", "x");
    store.set_scheduled(block, Some("2026-04-13")).unwrap();
    store.set_scheduled(block, None).unwrap();

    let page = store.journal_page("2026-04-13").unwrap().unwrap();
    let row = &store.page_blocks(page.id).unwrap()[0];
    assert_eq!(row.marker, Some(Marker::Todo));
    assert_eq!(row.scheduled_date, None);
    assert!(contents(&store, "2026-04-13").is_empty());
}

#[test]
fn agenda_rows_name_the_page_they_came_from() {
    let mut store = store();
    let block = scheduled(&mut store, "2026-04-11", "from saturday");
    store.set_scheduled(block, Some("2026-04-13")).unwrap();

    let rows = store.agenda("2026-04-13", 7).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].journal_date.as_deref(), Some("2026-04-11"));
    assert_eq!(rows[0].page_name, "2026-04-11");
}

#[test]
fn a_malformed_schedule_date_is_refused() {
    let mut store = store();
    let block = scheduled(&mut store, "2026-04-13", "x");
    assert!(store.set_scheduled(block, Some("13/04/2026")).is_err());
}
