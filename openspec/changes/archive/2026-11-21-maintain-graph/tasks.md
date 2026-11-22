# Tasks

## 0. Prerequisite

- [x] 0.1 Confirm `harden-note-store` is implemented and merged, since this change extends the same worker, store, and startup paths; verify `openspec list --all` or `git log` shows it, and stop to apply it first if it is still only planned.

## 1. Page rename (core)

- [x] 1.1 Add the `name_key` and strict-date helpers to `lognotes-core/src/store.rs` and validate a rename target: trimmed, non-empty, no `[`, `]`, or newline, not a strict date, not another page's key, page not a journal day. Verify unit tests for every refusal.
- [x] 1.2 Add `Action::RenamePage { page, from, to, changes }` that renames the page row first, then writes each changed block and reindexes, and whose inverse swaps names and texts in the same order; verify a rename with links and its undo both leave one page and the right links.
- [x] 1.3 Implement `Store::rename_page(page_id, new_name)` to scan every block, rewrite `[[Old]]` to `[[New]]`, rewrite `#old` to `#new` or `[[New Name]]` when the new name cannot be a tag, skip code and external URLs, and return how many blocks changed and how many plain mentions remain. Verify tests for links, tags, code, case-only rename, and mentions.
- [x] 1.4 Add tests that a successful rename is exactly one undo step and that a duplicate, date, journal, malformed, or empty target changes nothing; verify `cargo test -p lognotes-core`.
- [x] 1.5 Update the command/query tables and the link-identity paragraphs in `lognotes-core/SPEC.md`; verify the document matches `rename_page` and its tests.

## 2. Page delete (core)

- [x] 2.1 Add `Action::DeletePage { page, blocks }` and its restore inverse, capturing the page row (including favourite and visit order) and all blocks in id order; verify deleting the page and undoing restores the page and every block.
- [x] 2.2 Implement `Store::delete_page(page_id)` refusing journal days, missing pages, and pages with any `block_link` row, returning the linked count; allow deletion when only plain mentions remain. Verify tests for unlinked delete, linked refusal, mention-only delete, and journal refusal.
- [x] 2.3 Update the page command table in `lognotes-core/SPEC.md`; verify it matches `delete_page` and its tests.

## 3. Page actions (bridge and view)

- [x] 3.1 Add `Job::PageAction` for rename and delete, `Outcome.note`, and `PageView.linked_count`; add the `page_linked` property and return to the journal when a named page disappears after undo or redo. Verify bridge unit tests for success, refusal, and the missing-page fallback.
- [x] 3.2 Add `rename_page` and `delete_page` invokables, and a page-header menu visible only for named pages: rename dialog, delete confirmation with the linked count, and the result note at the bottom of the column. Verify `cargo test -p lognotes-qt` and the Settings note still works.
- [x] 3.3 Add livetest steps: rename a page that is linked from another block and confirm the rendered link changes, undo the rename, refuse deleting a linked page, delete an unlinked page, and return to the journal. Verify `make livetest`.
- [x] 3.4 Update the page-lifecycle paragraphs in `lognotes-qt/SPEC.md`; verify they match the dialogs and the note behavior.

## 4. Block move (core)

- [x] 4.1 Extend `Action::SetPosition` with `page_id`, read the old page/parent/key in `apply_action`, and update `indent` and `outdent` to pass the current page; verify all existing indent/outdent tests still pass.
- [x] 4.2 Add `Store::move_block(block, page, after)`, `move_up`, and `move_down`, with a cycle check that refuses moving a block into its own descendant; verify tests for sibling reorder, cross-page subtree move, refusal, and one-step undo.
- [x] 4.3 Update the command table in `lognotes-core/SPEC.md` and the ordering-keys paragraph; verify the document matches the new move commands.

## 5. Block move (view)

- [x] 5.1 Add bridge commands and invokables for move up/down, move after an anchor, and move to a named page or day; verify bridge unit tests using a store session.
- [x] 5.2 Add Alt+Up and Alt+Down to the delegate key handler, keeping the caret in the moved block and leaving modifiers with TextEdit; verify with a livetest step that reorders twice and undoes once.
- [x] 5.3 Add a drag handle visible on hover with row drop areas that compute before/after, dispatching one move on drop; verify a livetest or directed drag test and keep keyboard reorder working if the compositor defeats drag.
- [x] 5.4 Add the "Move to page…" picker over `page_names` and move the block with its subtree to the end of the chosen page or day; verify a livetest step.
- [x] 5.5 Update the block-arrangement paragraphs in `lognotes-qt/SPEC.md`; verify they match the keys, drag, and picker.

## 6. Folding

- [x] 6.1 Add `child_count` to `BlockRow`, compute it in `page_blocks`/`push_rows`, skip recursion for a collapsed block, and add `Store::set_collapsed` outside the undo history; update every `BlockRow` constructor and verify `cargo test -p lognotes-core`.
- [x] 6.2 Add the `collapsed`, `foldable`, and `children` roles, draw a chevron only when foldable, and clamp focus to the folded block when the caret is in one of its children; verify bridge tests and a livetest fold/unfold.
- [x] 6.3 Add tests that folding hides descendants, survives a fresh snapshot and a reopened store, and does not change undo; verify `cargo test -p lognotes-core -p lognotes-qt`.
- [x] 6.4 Update the folding paragraph and known limitations in `lognotes-qt/SPEC.md`; verify they match the persisted state and the focus rule.

## 7. Block navigation

- [x] 7.1 Add Up/Down handling to the editor's key handler: Up on the first line moves to the end of the previous editable row, Down on the last line moves to the start of the next, with no selection and no menu open. Verify a livetest typing after each move to prove no keystroke is lost.
- [x] 7.2 Add the editable-row helpers that skip headings, agenda rows, reference rows, and result pages, and leave the key alone at the first or last editable block; verify livetest steps across a day heading and into the agenda.
- [x] 7.3 Update the keyboard paragraphs in `lognotes-qt/SPEC.md`; verify they match the first-line/last-line rule and the menu guards.

## 8. Link autocomplete

- [x] 8.1 Add `markdown::open_link(text, byte_offset)` returning the unclosed `[[` start and query, skipping inline code and fences; verify unit tests for unclosed, closed, multiple, and code cases.
- [x] 8.2 Add the bridge `link_query(text, position_utf16)` with the Qt-to-byte conversion, and carry `page_names` as JSON in the snapshot and a model property, escaped with `json_string`; verify bridge unit tests and that a created, renamed, or deleted page changes the list.
- [x] 8.3 Add the completion popup: anchor at the caret, filter `page_names`, exact/prefix first, eight rows, Up/Down/Enter/Esc, click to commit, no focus steal, and `[[Name]]` insertion with the caret after it. Verify a livetest that types `[[My`, chooses a page, and saves.
- [x] 8.4 Add create-on-commit for a non-empty query with no match, leave typed text untouched on Esc, and prove no popup opens inside inline code or a fence; verify livetest and core tests.
- [x] 8.5 Update the `[[` completion paragraphs in `lognotes-qt/SPEC.md`; verify they match the popup and insertion rules.

## 9. Journal date navigation

- [x] 9.1 Expose a strict-date check from core, make `Journal::open_page` skip `EnsurePage` for dates, and make the page snapshot treat a missing date as a day page with a placeholder; verify bridge unit tests for an unwritten day with no row.
- [x] 9.2 Add the `open` purpose to the existing date popup (no `Clear`, OK opens the day) and a date control in the journal bar; verify a livetest opening a past and a future day.
- [x] 9.3 Add tests that typing on an unwritten day creates the page and block together, that a written day shows blocks and references without the agenda, and that Journal returns to the same column position; verify core, bridge, and livetest.
- [x] 9.4 Update the journal-navigation paragraphs in `lognotes-qt/SPEC.md`; verify they match the lazy day rule and the picker.

## 10. Integration verification

- [x] 10.1 Run `make test` and fix every failure; verify the whole workspace passes.
- [x] 10.2 Run `make livetest` and verify the existing scenario plus every new step passes with screenshots.
- [x] 10.3 Walk the five spec files scenario by scenario against the implementation; verify each has a test, a livetest step, or a documented manual check.
- [x] 10.4 Update both `SPEC.md` files, `architecture.md`, and the known limitations with the final behavior, including the recorded Backspace-merge omission; verify no stale claim from this change remains.

## Workflow follow-up

- Archive this change after review and verification, then check the archived specs.
