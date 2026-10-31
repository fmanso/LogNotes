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
re-open after an error), `Run` (an optional command, then a fresh snapshot of the loaded dates), and
`Flush` (a command plus a completion signal, for writes that must land before the process exits). It
delivers results by queueing a closure back onto the object's thread, so model resets and property
changes only ever happen on the GUI thread. Commands are `Update`, `Materialize`, `Submit`, `Indent`,
`Outdent`, `Delete`, `Undo`, `Redo` — each a thin translation to exactly one core call, except `Submit`,
which is a single command so that pressing Enter is a single undo step.

An empty `Run` (no command) is how `loadMore` asks for the next chunk: the bridge extends its own date
list by `CHUNK_DAYS` (14) and asks for a snapshot of the larger window.

## View behaviour worth knowing

- **Ctrl+Z has to be handled in the delegate.** `TextEdit` swallows it for its own per-widget undo, which
  knows nothing about block creation, indentation, or other days, so the delegate's `Keys` handler claims
  it first, flushes any unsaved text, then calls the model's undo/redo. The window-level `Shortcut`s only
  cover the case where focus sits outside a block.
- **cxx-qt does not camelCase Rust identifiers for QML.** `submit_block` is `submit_block` in QML, not
  `submitBlock`. A mismatch compiles and fails only when the key is pressed, so a unit test scans the
  QML for `journal.<name>(` calls and asserts each is a declared `#[qinvokable]`.
- **The debounce is in QML** (250 ms), because the timer is a view concern and Qt already has one. Only
  structural keys — Enter, Tab, Shift+Tab, Backspace on an empty block — flush immediately.
- **Every save resets the model**, which destroys the focused delegate. Two things follow, and both are
  load-bearing: the model exposes a `revision` counter bumped on each applied snapshot (the row count does
  not change when a block is merely edited, so `countChanged` cannot be the trigger), and the window keeps
  `focusRow`/`focusCursor` and restores them on every revision.
- **A blur caused by that rebuild must not save again.** The delegate's focus-out handler would otherwise
  queue an edit, whose snapshot resets the model once more — and since the row count is unchanged the
  second time, no restore fires and the caret is simply gone. A `rebuilding` flag, set when any command is
  dispatched and cleared when its snapshot arrives, suppresses exactly that save.
- **Backspace on an empty block deletes; on a day's first block it simply does nothing** beyond clearing
  text, because the core refuses to merge across pages.
- **Closing flushes the pending edit.** Typing is debounced, so closing the window would otherwise drop
  whatever was typed in the last 250 ms. The window keeps the unsaved text per row and, on close, calls
  `save_now` for each: that job carries a completion channel, so the close waits for the worker to commit
  (bounded at 2 s) instead of racing the process exit.
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

## Live testing

The window can be driven without a person at the keyboard, which is how its input handling gets checked:

```
cargo run -p lognotes-qt --features driver -- --livetest
```

It writes to a throwaway database under `$TMPDIR/lognotes-livetest`, loads the same `main.qml` the app
loads, and drives it through the real input path: typing, Enter, Tab, Shift+Tab, Ctrl+Z, a mouse click on
a block, wheel scrolling to pull in an older chunk, and finally a close with a pending debounce. Each step
is checked against the stored rows and depths, and every step saves a PNG so a human (or a reviewing agent)
can see the rendered window rather than trust a passing assertion.

`--livetest-error` covers the other half: it points the store at an unopenable path and asserts the error
pane appears with no rows, screenshotting it. That run is what caught the error heading being drawn in the
theme's default near-white on a cream background.

Qt key codes are a trap worth stating once: `0x01000007` is Delete, not End (`0x01000011`). A wrong
constant in the harness looks exactly like an application bug.

The `driver` feature is the boundary: without it, `src/driver.rs`, `driver_shim.cpp` and `livetest.rs` are
not compiled and the shipped binary carries no test hook. `lognotes-qt/driver_shim.h` is the only C++ in
the project that is not generated.
