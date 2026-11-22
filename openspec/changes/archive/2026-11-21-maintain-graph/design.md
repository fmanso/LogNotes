# Design

## Context

See `proposal.md` — Why. This change builds on `harden-note-store`: that phase adds the migration, backup, and startup guarantees this one assumes, but it does not change the page/block model.

The constraints that shape this:

- Links are derived from block text. `markdown::scan` returns `LinkRef { target, kind, start, end }` for `[[pages]]`, `#tags`, and external URLs, skipping code spans and fences; `reindex` turns them into `block_link` on every text write.
- A page row is an address. `page.name_key` is the trimmed lowercase identity, unique; `ensure_page_on` maps a strict date to a journal day.
- The worker owns the one `Store`; the GUI holds flat `Row`s. Every command answers with a full snapshot.
- `block.collapsed` and `BlockRow.collapsed` exist but nothing reads or writes them. `Action::SetPosition` already stores a block's old parent and order key.
- There is no page update or page delete anywhere.

## Goals / Non-Goals

**Goals:**

- Rename and delete pages without leaving stale links or a graph that silently re-creates what was deleted.
- Move a block with its subtree, fold a block, and navigate between blocks from the keyboard.
- Make `[[` completion and date navigation feel like the rest of the app: keyboard-first, one undo per edit, no hidden writes.
- Keep every change undoable except view state (fold, star, visit).

**Non-Goals:**

- Block multi-select, page merge, or a page-trash.
- Backspace merging a block into its previous sibling: recorded as a known limitation, not implemented here (see Decisions).
- A persisted undo history, or undo across a restart.
- Attachments, query language, or graph view.

## Decisions

### Page rename: rename the row first, then rewrite references

A rename is one transaction and one dedicated action:

```text
1. validate the new name
2. for every block:
     scan its text
     for links/tags whose target key == the old key:
       replace [[Old]] with [[New]]
       replace #old with #new, or with [[New Name]] when New cannot be written as a tag
       (whitespace, #, or brackets)
3. Action::RenamePage { page, from, to, changes }:
     rename the page row to the new name and key first
     then write each changed block's text and reindex it
4. commit; the action's inverse is the same action with the names and texts swapped
```

The page row is renamed **before** the contents are rewritten: `SetContent` calls `reindex`, and `reindex` calls `ensure_page_on(new_name)`. If the old row still carried the old key, that would create a second page beside the one being renamed.

This is deliberately **not** a composite of `SetPageName` plus `SetContent`: `Composite` applies its inverse in reverse order, so undo would restore the old block texts while the page row still had the new name. `reindex` would then create a page for the old name, and the final rename back to it would hit the unique key. The dedicated action reverses both halves in the only order that works: rename first, then reindex the restored text.

Replacement runs from the end of each block so byte offsets stay valid. `scan` already excludes code, so a fence or inline code sample keeps its brackets. External URLs (`LinkKind::Url`) are untouched.

Validation: trim; non-empty; no `[`, `]`, or newline; not a strict `YYYY-MM-DD`; not the name of another page under `name_key`; the page must not be a journal day. A case-only rename is allowed because the key is the page's own.

**Plain mentions:** they are not links, so they are not rewritten. Before the rename, `references(page)` counts `ReferenceKind::Mention`; the result note says how many prose mentions were left behind. This is deliberately honest rather than silently rewriting prose.

**Alternatives:** delete and recreate the page (loses id, star, visit order, and existing link rows); a raw string replace (touches code and misses case); a regex over `[[…]]` (misses tags and fences). The scan is already the one reading of block text, so the rename uses it.

**Undo and the view:** after undoing a rename, the page in view no longer exists under its name. The snapshot has no page row; the worker switches a missing **named** page back to the journal rather than showing an empty page you could type into and accidentally recreate. A missing **date** page is different: that is the lazy day case and stays open.

### Page delete: refuse while explicitly linked

Deleting is allowed only for a named page with no `block_link` rows pointing at it. If any block still contains `[[Target]]` or `#target`, the delete is refused and the count is reported; the user removes those references first. Deleting anyway would either leave a link that recreates the page on the next save or require rewriting prose, both worse than a clear refusal.

Plain mentions do not block: they are not references and cannot recreate a page. Journal days are addresses, not documents; they are not deletable.

The delete captures the page row (including `favourite` and `visit_order`) and every block on it, deletes the page (cascade removes blocks and links), and returns one inverse that restores the page with its id and its blocks. Blocks are restored in id order so a parent exists before its children. The rest is derived and rebuilt.

**Alternatives:** strip links from the text (surprising, destructive); leave links and let extraction recreate (the page never really goes away); soft-delete (a second state the model would have to carry everywhere).

### Page actions are their own job, with a note

Rename and delete change what page the view should show, so they do not fit the block `Command` path, whose snapshot is built for the mode the GUI sent. Add a `Job::PageAction` carrying the current mode:

```text
Rename: perform, on success report mode Page(new); on refusal keep the old mode
Delete: perform, on success report mode Journal; on refusal keep the old mode
```

`Outcome` gains `note: Option<String>`. `apply` writes it to the existing `note` property; QML shows it at the bottom of the column (Settings keeps using the same property). Refusals are notes, not `error`, so the column never disappears because a page could not be renamed. Store failures still use `error`.

`PageView` gains `linked_count`, carried to a `page_linked` property, so the delete dialog can say "N linked references" before the user tries.

### Move: extend `SetPosition` with the page

`Action::SetPosition` grows a `page_id` field. `apply_action` reads the old `(page_id, parent_id, order_key)` triple, updates all three, and returns the inverse. `indent` and `outdent` pass the current page; cross-page moves pass the target.

Core gains:

- `move_block(block, page, after)`: `after` is the sibling to sit behind; its parent is the new parent, and the key is between `after` and its next sibling. `after = None` appends at the top level of the target page.
- `move_up(block)` / `move_down(block)`: swap with the previous/next sibling by choosing the key on the other side.
- A cycle check: the target parent must not be the block itself or one of its descendants.

Content is unchanged, so `block_link` and `block_fts` need no reindex; only page ownership and order move. The old page and the new page both get the next snapshot.

**Alternatives:** a separate `Move` action (duplicates the old-position logic); delete and re-insert (new ids break focus, links, and the inverse); renumber all siblings (against the fractional-key design).

Keyboard: Alt+Up and Alt+Down in the delegate, handled before `TextEdit` sees them. Drag: a handle visible on hover, with the block id as the drag payload and row drop areas computing before/after; the drop dispatches one move. Cross-page: a "Move to page…" picker over the snapshot's page names, because the sidebar lists only named pages, not days.

### Folding: persisted view state, outside undo

`Store::set_collapsed(block, bool)` writes the column directly, like a star or a visit, and does not push history. `page_blocks` changes:

- each row computes `child_count` from the parent map;
- `push_rows` includes a collapsed block but does not recurse into its children.

`BlockRow` gains `child_count`; the bridge copies `collapsed`, `foldable` (`child_count > 0`), and the count into the row roles. QML draws a chevron only when foldable and toggles through the invokable.

When the caret is inside a child of the block being folded, QML walks up the flat rows (by depth) and moves `focusRow` to the folded block before the snapshot arrives. Otherwise the focused delegate disappears with the children. The existing revision-based focus restore then lands on the parent.

**Alternatives:** hide children in QML at height 0 (keeps dead delegates and tab stops); store fold state in the settings table (loses which block is folded); make fold undoable (one undo should not unfold a view).

### Block navigation: delegate key handling

The editor's `Keys.onPressed` gains Up and Down, but only when there is no selection and no completion or slash menu is open:

```text
Up   when text.lastIndexOf("\n", cursor - 1) < 0   -> previous editable row, caret at its end
Down when text.indexOf("\n", cursor) < 0           -> next editable row, caret at 0
```

"Editable" means `isBlock` (a block or a placeholder). Headings, agenda rows, reference rows, and result pages are skipped. The focus move uses the existing `focusRow`/`focusCursor` and `focusBlock` path, so the current editor's pending text is flushed by the same blur handling as a click. If no editable row exists in that direction, the key is left to `TextEdit` (which does nothing at the edge).

**Why QML:** line boundaries and caret positions are view data; core knows blocks, not carets. **Alternative:** a core command to "focus previous block" would still need QML to place the caret and would double the round trip.

### `[[` completion: one scanner query, one local list

Core adds:

```text
markdown::open_link(text, byte_offset) -> Option<OpenLink { start, query }>
```

It finds the last `[[` before the offset that has no closing `]]`, using `scan` to reject offsets inside code spans and fences. `start` is a byte offset; the query is the text between the brackets and the caret.

The bridge adds `link_query(text, position_utf16)`, converting the Qt caret to bytes with the same helper `link_at` uses. The snapshot carries `page_names` as JSON (escaped with the existing `json_string`): every page and written day, sorted by key. This resurrects the `page_names` field the old SPEC described; the Pages popup was removed without removing the autocomplete need.

QML mirrors the slash menu's mechanics: a `focus: false` popup anchored at `editor.positionToRectangle(cursor)`, filtered locally as the user types, exact/prefix first, capped at eight. Up/Down/Enter/Esc are claimed in the delegate before `TextEdit`. Enter replaces the typed token with `[[Name]]`, keeps one closing pair if the text already had one, and puts the caret after it. With no match and a non-empty query, it closes as `[[query]]`; extraction creates the page on the debounced save. Esc closes and leaves every typed character.

The popup does not open inside code because `open_link` returns nothing there. The page list comes from the same snapshot, so a page created by the last save is offered next time.

**Alternatives:** a worker query per keystroke (a round trip per character, and the search job is debounced at 120 ms); filtering `page_names` only for exact prefixes (misses the middle of a name). The local list is the same trade-off the old Pages popup made, at personal scale.

### Date navigation: a date page can exist before its row

Core exposes `is_date_name(name)` (the existing strict round-trip rule). `Journal::open_page`:

- for a strict date: set `Mode::Page(date)` and dispatch no `EnsurePage`; the day stays unwritten;
- for a named page: keep the current ensure, because a link click must land somewhere you can type.

The snapshot's page branch handles a missing row: when the name is a date, it sets `journal_date = Some(name)` so the header renders as the day and the empty page gets a placeholder. When the name is not a date and the row is missing, the worker falls back to the journal (the undo-rename case).

The first keystroke on the placeholder goes through `Materialize` with the view's destination; core's `ensure_page` maps a strict date to `ensure_journal_page`, so the row and block are created together. A written day simply finds its row and shows blocks and references; the agenda stays in the journal column.

The existing date popup gains a third purpose, `open`: it has no `Clear` button, and OK calls `open_page(iso)` instead of writing the date into a block.

**Alternatives:** create the page on open (violates "reading never writes" and leaves empty rows); a separate day mode (duplicates `Mode::Page` for dates, which already carry the date in the header).

### Backspace merge is recorded, not built

The proposal allows either implementing the merge or recording the omission. It is omitted: merging needs a composite of text, children, marker and schedule policy, plus an outdent fallback at the first child, and none of the capabilities in this change require it. The current behavior stays: Backspace on an empty block deletes it; at the start of a non-empty block it does nothing. `lognotes-qt/SPEC.md`'s known-limitations section names the omission.

## Risks / Trade-offs

- **A rename rewrites user text** → it only touches explicit links/tags outside code, and one undo restores everything. Plain mentions are left and counted, not guessed at.
- **A tag becomes a link when the new name has spaces** → the reference survives, but the syntax changes visibly. The result note explains the rewrite, and the alternative (a broken tag) is worse.
- **Deleting is refused while linked** → this can feel strict, but the message names the references and nothing is lost. Stripping links silently would be worse.
- **Extending `SetPosition` touches indent/outdent** → both already pass parent/key; add page and keep their tests; add a cross-page move test for page ownership.
- **Dragging with `cacheBuffer: 0` and model resets** → the drag payload is a block id, not a row index, and nothing is written until drop; the livetest covers a reorder. If drag proves unreliable under the compositor, keyboard reorder and the page picker remain complete paths.
- **`page_names` JSON on every snapshot grows with the graph** → personal-scale names are small; the quick switcher already loads all pages for search. If it ever hurts, cap the completion list, not the snapshot.
- **Fold can hide the caret** → the fold path clamps focus to the folded block, and the revision-based restore already handles the rebuild.
- **Undo after rename leaves the old view** → a missing named page falls back to the journal; the old page still exists and can be opened from the sidebar or search.
- **Lazy date pages have no row to attach references to** → a day with no row has no references by definition; after the first write the row exists and the next snapshot shows them.

## Migration Plan

- No schema change: `collapsed` already exists, and page rename/delete use existing columns.
- The bridge and QML change together; the worker commands and invokables are additive except `Action::SetPosition`, which is internal.
- `harden-note-store` should land first, because this change adds page actions to the same worker and relies on its restore/backup behavior; the two touch different store operations and can be merged separately.
- After the behavior lands, update the command tables and known limitations in both `SPEC.md` files and the relevant sections of `architecture.md`.

## Open Questions

None that change the specs, the approach, or the task breakdown. The remaining details are cosmetic: the exact chevron glyph and the completion popup's width.
