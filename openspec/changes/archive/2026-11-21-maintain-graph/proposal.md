# Proposal

## Why

The app is good at creating notes and weak at tidying them. Over years of daily use the graph accumulates junk and the keyboard workflow has holes:

- **Pages cannot be renamed or deleted.** There is no `rename_page` and no `DELETE FROM page` anywhere; a typo'd `[[link]]` creates a page that is permanent. Links are derived from block text (`architecture.md:56-58`), so a rename is not a row update — it must rewrite the references.
- **Blocks cannot be moved or reordered.** The core SPEC documents `move(block, page, after)` (`lognotes-core/SPEC.md:117`), but the store only has `indent` and `outdent`. This is the most conspicuous outliner gap.
- **Arrow keys do not move between blocks.** Up/Down are handled only inside menus (`main.qml:914,1099,1700,1800`); a keyboard-first outliner without block navigation is felt every day.
- **Folding is inert.** `block.collapsed`, `BlockRow::collapsed`, and the schema column exist, but nothing writes them, `push_rows` ignores them (`store.rs:1199-1223`), and there is no `set_collapsed`.
- **`[[` autocomplete is missing** (`lognotes-qt/SPEC.md:335`). Together with the missing rename/delete, typos have no cheap correction.
- **There is no way to jump to a date.** Only written days are reachable through search or a date link; an unwritten past day or a future day cannot be opened on purpose.

Why now: `harden-note-store` protects the file; this phase protects its usefulness as the graph grows. Without it, daily use drifts toward junk pages and manual workarounds.

## What Changes

- Rename a page: rewrite its `[[links]]` and `#tags` across block text as one undo step; keep plain-text mentions untouched and report them.
- Delete a page: decide the policy when other blocks still reference it (refuse, strip references, or leave the links to recreate it), then remove the page and its blocks in one undoable action.
- Move a block with its subtree within a page, across positions, and across pages; expose keyboard reordering and a drag affordance.
- Move the caret between blocks with Up/Down at text boundaries, so a block can be reached without the mouse.
- Make folding real: persist `collapsed`, omit a folded block's children from the snapshot, and draw a fold control.
- Autocomplete `[[` over existing page names while typing, with create-on-commit, so links stop being spelling tests.
- Add a pick-a-date control to the journal, opening an unwritten day without creating a row until something is typed.
- Decide whether Backspace at the start of a block merges it with the previous one, and either implement the merge or record the omission.

## Capabilities

### New Capabilities

- `page-lifecycle`: renaming and deleting pages while keeping derived links coherent.
- `block-arrangement`: moving, reordering, and folding blocks.
- `block-navigation`: moving the caret between blocks from the keyboard.
- `link-authoring`: writing links with `[[` completion over known pages.
- `journal-navigation`: reaching any journal day, written or not, by date.

### Modified Capabilities

None. No specs exist yet.

## Impact

- `lognotes-core`: page rename/delete, block move, fold persistence, snapshots that honour `collapsed`, link re-extraction after a rename.
- `lognotes-qt`: bridge commands, row roles and model rebuilds, QML delegate key handling, fold and autocomplete UI.
- Tests: rename rewrites links as one undo step; delete policy; cross-page move restores the subtree; fold persists across snapshots; navigation at text boundaries; autocomplete creation.
- The derived link table and FTS index must stay consistent with rewritten text.
- Both `SPEC.md` files need updating; the documented `move`/`set_collapsed` commands become real.
