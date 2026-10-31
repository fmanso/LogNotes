---
id: lognotes-qt
type: module-design
status: active
title: lognotes-qt — journal model and QML column
parent: architecture
tags: [rust, qt6, qml, cxx-qt]
---

# lognotes-qt

Turns store snapshots into a flat `QAbstractListModel` the journal column can render, and marshals what
the user does back to the worker thread that owns the store.

## Responsibility

One QObject, `Journal`, plus one QML window. It translates: rows for the view, commands for the core. It
holds no note data of its own beyond the rows currently on screen, and it never constructs SQL.

## Boundary

- **Depends on:** `lognotes-core`, `cxx-qt` / `cxx-qt-lib`, `chrono` for date arithmetic on chunks.
- **Forbidden:** `rusqlite` (the bridge must not own or reach a `Connection`), file-system writes of its
  own, and any second writer thread.
- **Depended on by:** nothing. The crate's one binary is the application.

The store is opened *inside* the worker thread, from a path `main` publishes before the window loads, so
the GUI thread never has an open handle to pass around — or to leak into a delegate.

## Model rows

`rowCount`/`data` expose one flat list, rebuilt wholesale from a snapshot. A row is one of three kinds,
carried in the `kind` role:

| kind | meaning |
| --- | --- |
| `heading` | A journal date. Carries `date`; the QML formats it for display. |
| `block` | A persisted block, carrying `blockId` and `depth`. |
| `placeholder` | The block an untouched day shows so the cursor has somewhere to land. `blockId` is `-1` and **nothing is written** until the first keystroke. |

Roles are `kind`, `text`, `depth`, `date`, `blockId`, `placeholder`. Dates stay ISO `YYYY-MM-DD` in the
model and are formatted in QML — the core never learns a locale, and the view never parses a date.

Placeholder rows are why scrolling a year into the past cannot bloat the database: only a keystroke
materialises a day (`ensure_journal_page` + insert). This is a view-layer rule, not a schema one.

## Threading and delivery

The worker loop owns `Store` for its lifetime and receives `Job`s over a channel: `Start` (open, or
re-open after an error) and `Run` (an optional command, then a fresh snapshot of the loaded dates). It
delivers results by queueing a closure back onto the object's thread, so model resets and property
changes only ever happen on the GUI thread. Commands are `Update`, `Materialize`, `Submit`, `Indent`,
`Outdent`, `Delete`, `Undo`, `Redo` — each a thin translation to exactly one core call, except `Submit`,
which is a single command so that pressing Enter is a single undo step.

An empty `Run` (no command) is how `loadMore` asks for the next chunk: the bridge extends its own date
list by `CHUNK_DAYS` (14) and asks for a snapshot of the larger window.

## View behaviour worth knowing

- **cxx-qt does not camelCase Rust identifiers for QML.** `submit_block` is `submit_block` in QML, not
  `submitBlock`. A mismatch compiles and fails only when the key is pressed, so a unit test scans the
  QML for `journal.<name>(` calls and asserts each is a declared `#[qinvokable]`.
- **The debounce is in QML** (250 ms), because the timer is a view concern and Qt already has one. Only
  structural keys — Enter, Tab, Shift+Tab, Backspace on an empty block — flush immediately.
- **Every save resets the model**, which destroys the focused delegate. The window therefore tracks
  `focusRow` and `focusCursor` and restores them after the rebuild; this is the reason the delegate
  recreates the caret rather than the model preserving selection.
- **Backspace on an empty block deletes; on a day's first block it simply does nothing** beyond clearing
  text, because the core refuses to merge across pages.
- **Nothing here renders task markers or links yet** — the roles exist in the schema, but `data` does not
  expose them. Folding is likewise not implemented: every block renders expanded.

## Failure

`start()` sets `error` and leaves `ready` false when the store refuses to open; QML then shows the error
pane (message, path, Retry, Quit) and hides the column. `ready` is the only gate on rendering rows, so a
broken database can never look like an empty journal.

## Known limitations

- Editing is plain text: markdown is not rendered, nothing is styled, and link autocompletion does not
  exist yet.
- Blocks cannot be reordered (only indented, outdented, created, edited, deleted).
- The focus restoration is index-based; a keypress that changes the row count between snapshot and
  rebuild can land the caret one row off.
- `cursorPosition` from QML is a UTF-16 offset while the core splits at UTF-8 byte boundaries; they agree
  for BMP text and diverge for characters outside it (emoji), where a split lands on the nearest boundary.
