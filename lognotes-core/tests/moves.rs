//! Moving a block with its subtree, without crossing the page boundary by accident.

use lognotes_core::{Error, Store};

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
fn up_and_down_swap_with_the_neighbour_and_stop_at_the_ends() {
    let mut store = store();
    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    let a = store.insert_block(day, None, "a").expect("a");
    let b = store.insert_block(day, None, "b").expect("b");
    let c = store.insert_block(day, None, "c").expect("c");

    store.move_up(c).expect("c up");
    assert_eq!(contents(&store, day), ["a", "c", "b"]);

    store.move_down(a).expect("a down");
    assert_eq!(contents(&store, day), ["c", "a", "b"]);

    // The ends are no-ops.
    let before = contents(&store, day);
    store.move_up(c).expect("first is a no-op");
    store.move_down(b).expect("last is a no-op");
    assert_eq!(contents(&store, day), before);
    assert!(a > 0 && b > 0 && c > 0);
}

#[test]
fn a_block_moves_after_an_anchor_on_the_same_page() {
    let mut store = store();
    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    let a = store.insert_block(day, None, "a").expect("a");
    store.insert_block(day, None, "b").expect("b");
    let c = store.insert_block(day, None, "c").expect("c");

    store.move_block(c, day, Some(a)).expect("move c after a");
    assert_eq!(contents(&store, day), ["a", "c", "b"]);
}

#[test]
fn before_and_after_anchors_place_a_block_on_either_side() {
    let mut store = store();
    let day = store.ensure_journal_page("2026-11-02").expect("a day");
    let a = store.insert_block(day, None, "a").expect("a");
    let b = store.insert_block(day, None, "b").expect("b");
    let c = store.insert_block(day, None, "c").expect("c");

    store.move_block_before(c, a).expect("c before a");
    assert_eq!(contents(&store, day), ["c", "a", "b"]);

    store.move_block_after(c, b).expect("c after b");
    assert_eq!(contents(&store, day), ["a", "b", "c"]);
}

#[test]
fn a_block_can_be_sent_to_a_page_by_name() {
    let mut store = store();
    let from = store.ensure_page("From").expect("from");
    let to = store.ensure_page("To").expect("to");
    let block = store
        .insert_block(from, None, "traveller")
        .expect("a block");

    store
        .move_block_to_page(block, "To")
        .expect("the move by name");
    assert_eq!(contents(&store, from), Vec::<String>::new());
    assert_eq!(contents(&store, to), ["traveller"]);
}

#[test]
fn a_cross_page_move_takes_the_subtree_and_undo_returns_it() {
    let mut store = store();
    let from = store.ensure_page("From").expect("from");
    let to = store.ensure_page("To").expect("to");
    let parent = store.insert_block(from, None, "parent").expect("parent");
    let child = store
        .insert_block(from, Some(parent), "child")
        .expect("child");
    store.indent(child).expect("nest the child");

    store
        .move_block(parent, to, None)
        .expect("the cross-page move");

    assert_eq!(contents(&store, from), Vec::<String>::new());
    let moved = store.page_blocks(to).expect("the target page");
    assert_eq!(
        moved
            .iter()
            .map(|block| (block.content.as_str(), block.depth))
            .collect::<Vec<_>>(),
        [("parent", 0), ("child", 1)]
    );
    assert!(
        moved
            .iter()
            .all(|block| store.page_of_block(block.id).expect("its page").id == to)
    );

    store.undo().expect("one undo");
    assert_eq!(contents(&store, from), ["parent", "child"]);
    assert_eq!(contents(&store, to), Vec::<String>::new());
}

#[test]
fn a_block_cannot_be_moved_into_its_own_child() {
    let mut store = store();
    let from = store.ensure_page("From").expect("from");
    let parent = store.insert_block(from, None, "parent").expect("parent");
    let child = store
        .insert_block(from, Some(parent), "child")
        .expect("child");
    store.indent(child).expect("nest the child");

    assert!(matches!(
        store.move_block(parent, from, Some(child)),
        Err(Error::MoveIntoSelf)
    ));
    assert_eq!(contents(&store, from), ["parent", "child"]);
}
