//! Renaming and deleting pages without leaving stale links behind.

use lognotes_core::{Error, Marker, Store};

fn store() -> Store {
    Store::open_in_memory().expect("a store")
}

fn contents(store: &Store, page: i64) -> Vec<String> {
    store
        .page_blocks(page)
        .expect("blocks")
        .into_iter()
        .map(|block| block.content)
        .collect()
}

#[test]
fn a_rename_rewrites_links_and_tags_and_undoes_in_one_step() {
    let mut store = store();
    let project = store.ensure_page("MyProject").expect("a page");
    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    let linked = store
        .insert_block(day, None, "see [[MyProject]] and #MyProject")
        .expect("a block");
    let code = store
        .insert_block(day, None, "`[[MyProject]]`\n```\n[[MyProject]]\n```")
        .expect("a code block");
    let mention = store
        .insert_block(day, None, "the MyProject notes")
        .expect("a mention");

    let renamed = store
        .rename_page(project, "Project Alpha")
        .expect("a rename");
    assert_eq!(renamed.blocks, 1, "only the linked block changed");
    assert!(
        renamed.mentions >= 1,
        "the plain mention is reported: {renamed:?}"
    );

    assert!(store.page_by_name("myproject").expect("a lookup").is_none());
    let now = store
        .page_by_name("project alpha")
        .expect("a lookup")
        .expect("the renamed page");
    assert_eq!(now.name, "Project Alpha");

    assert_eq!(
        store
            .page_blocks(day)
            .expect("blocks")
            .into_iter()
            .find(|block| block.id == linked)
            .expect("the linked block")
            .content,
        "see [[Project Alpha]] and [[Project Alpha]]"
    );
    assert_eq!(
        store
            .page_blocks(day)
            .expect("blocks")
            .into_iter()
            .find(|block| block.id == code)
            .expect("the code block")
            .content,
        "`[[MyProject]]`\n```\n[[MyProject]]\n```",
        "code is untouched"
    );
    assert_eq!(
        store
            .page_blocks(day)
            .expect("blocks")
            .into_iter()
            .find(|block| block.id == mention)
            .expect("the mention")
            .content,
        "the MyProject notes",
        "prose is untouched"
    );

    // One undo restores the page name and the rewritten text together.
    assert!(store.undo().expect("undo"));
    assert!(
        store
            .page_by_name("project alpha")
            .expect("a lookup")
            .is_none()
    );
    assert!(store.page_by_name("myproject").expect("a lookup").is_some());
    assert_eq!(
        store
            .page_blocks(day)
            .expect("blocks")
            .into_iter()
            .find(|block| block.id == linked)
            .expect("the linked block")
            .content,
        "see [[MyProject]] and #MyProject"
    );
}

#[test]
fn a_case_only_rename_updates_the_spelling_of_references() {
    let mut store = store();
    let page = store.ensure_page("my project").expect("a page");
    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    let block = store
        .insert_block(day, None, "[[my project]]")
        .expect("a block");

    store.rename_page(page, "My Project").expect("a rename");

    assert_eq!(
        store
            .page_blocks(day)
            .expect("blocks")
            .into_iter()
            .find(|row| row.id == block)
            .expect("the block")
            .content,
        "[[My Project]]"
    );
    assert_eq!(
        store
            .page_by_name("my project")
            .expect("a lookup")
            .expect("the page")
            .name,
        "My Project"
    );
}

#[test]
fn a_tag_stays_a_tag_when_the_new_name_can_be_one() {
    let mut store = store();
    let page = store.ensure_page("Old").expect("a page");
    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    let block = store.insert_block(day, None, "#Old").expect("a block");

    store.rename_page(page, "New").expect("a rename");

    assert_eq!(
        store
            .page_blocks(day)
            .expect("blocks")
            .into_iter()
            .find(|row| row.id == block)
            .expect("the block")
            .content,
        "#New"
    );
}

#[test]
fn a_rename_is_refused_for_a_duplicate_a_date_a_journal_or_a_bad_name() {
    let mut store = store();
    let page = store.ensure_page("Mine").expect("a page");
    store.ensure_page("Taken").expect("the other page");
    let journal = store.ensure_journal_page("2026-11-02").expect("a day");

    assert!(matches!(
        store.rename_page(page, "taken"),
        Err(Error::PageExists(_))
    ));
    assert!(matches!(
        store.rename_page(page, "2026-11-02"),
        Err(Error::BadRenameName(_))
    ));
    assert!(matches!(
        store.rename_page(journal, "Another day"),
        Err(Error::CannotRenameJournal)
    ));
    assert!(matches!(
        store.rename_page(page, "bad[name]"),
        Err(Error::BadRenameName(_))
    ));
    assert!(matches!(
        store.rename_page(page, "   "),
        Err(Error::BadRenameName(_))
    ));

    assert_eq!(
        store
            .page_by_name("mine")
            .expect("a lookup")
            .expect("the page")
            .name,
        "Mine"
    );
}

#[test]
fn deleting_an_unlinked_page_removes_it_and_one_undo_restores_it() {
    let mut store = store();
    let page = store.ensure_page("Gone").expect("a page");
    let first = store.insert_block(page, None, "one").expect("a block");
    store
        .set_marker(first, Some(Marker::Todo))
        .expect("a marker");

    store.delete_page(page).expect("the delete");
    assert!(store.page_by_name("gone").expect("a lookup").is_none());

    assert!(store.undo().expect("undo"));
    let back = store
        .page_by_name("gone")
        .expect("a lookup")
        .expect("the page");
    assert_eq!(contents(&store, back.id), ["one"]);
    assert_eq!(
        store.page_blocks(back.id).expect("blocks")[0].marker,
        Some(Marker::Todo)
    );
}

#[test]
fn deleting_is_refused_while_a_link_remains_and_mentions_do_not_block() {
    let mut store = store();
    let linked = store.ensure_page("Linked").expect("a page");
    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    let block = store.insert_block(day, None, "[[Linked]]").expect("a link");

    assert!(matches!(
        store.delete_page(linked),
        Err(Error::PageHasLinks(1))
    ));
    assert!(store.page_by_name("linked").expect("a lookup").is_some());

    store.delete_block(block).expect("remove the link");
    assert!(
        store.delete_page(linked).is_ok(),
        "the page can go once its last link is gone"
    );
    assert!(store.page_by_name("linked").expect("a lookup").is_none());

    let mentioned = store.ensure_page("Mentioned").expect("a page");
    store
        .insert_block(day, None, "the Mentioned notes")
        .expect("a mention");
    assert!(store.delete_page(mentioned).is_ok());
    assert!(store.page_by_name("mentioned").expect("a lookup").is_none());
}

#[test]
fn a_journal_day_is_not_deletable() {
    let mut store = store();
    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    assert!(matches!(
        store.delete_page(day),
        Err(Error::CannotDeleteJournal)
    ));
}
