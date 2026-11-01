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

It also draws what the text means, without ever rewriting it: each block editor is styled in place by a
syntax highlighter, a read-only row is rendered from escaped HTML, and a ```mermaid fence is drawn to an
SVG the view loads.

## Boundary

- **Depends on:** `lognotes-core`, `cxx-qt` / `cxx-qt-lib`, `chrono` for date arithmetic on chunks,
  `mermaid-rs-renderer` (default features off — the SVG path only) for fences, and Qt's SVG image plugin
  to draw them.
- **Forbidden:** `rusqlite` (the bridge must not own or reach a `Connection`), file-system writes of its
  own beyond the diagram cache, and any second writer thread.
- **Depended on by:** nothing. The crate's one binary is the application.

The C++ in this crate is two hand-written shims beside the generated glue: `markdown_shim.cpp` applies
formats to a text document and `driver_shim.cpp` drives the window under the `driver` feature. Everything
else is cxx-qt's.

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
| `agenda-header` | The `Scheduled` heading, present only when something is due. |
| `agenda-row` | A due block, spliced directly under today's blocks; carries the same `blockId`, so acting on it acts on the block. |
| `page-header` | The name of the page being viewed. |
| `reference-header` / `reference-row` | `Linked References` and the blocks that link this page. |
| `unlinked-header` / `unlinked-row` | The collapsed `Unlinked mentions (N)` group and its rows. |

Roles are `kind`, `text`, `depth`, `date`, `blockId`, `placeholder`, `marker`, `scheduled`, `source`,
`overdue`, `html`, `diagram`, `diagramError`. Dates stay ISO `YYYY-MM-DD` in the
model and are formatted in QML — the core never learns a locale, and the view never parses a date.

Two of those roles carry rendering rather than data: `html` is a read-only row's text already rendered
from the same scan the editor styles with (marks dropped, links carrying `page:` or a URL), and `diagram`
is the `file://` URL of the SVG drawn for that block's fence, with `diagramError` holding one line when
the fence did not parse.

Placeholder rows are why scrolling a year into the past cannot bloat the database: only a keystroke
materialises a day (`ensure_journal_page` + insert). This is a view-layer rule, not a schema one.

## Two views

The model shows either the journal column or one named page. `open_page(name)` / `show_journal()` set the
mode, which is carried to the worker with every job so each snapshot is built for the view that asked for
it: a page snapshot is its blocks plus `references(page)`; a journal snapshot is days plus the agenda.

The page list for the switcher rides along with each snapshot as a JSON array (`page_names`), so opening the
switcher costs no round trip. Switching views replaces every row, so QML sets `pendingViewFocus` and puts the
caret in the new view's first block once its snapshot lands — without that, the caret is nowhere and typing
after a switch goes into the void.

The collapsed state of the unlinked-mentions group is view-only state (`unlinkedExpanded`); the rows are
always in the model, at height 0 while collapsed. Keeping the model unaware of it means the toggle costs no
query.

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

Building a snapshot is also where diagrams are drawn. The worker keeps a cache keyed by the hash of each
fence's code, renders what it has not seen to `temp_dir()/lognotes-diagrams/<hash>.svg`, and puts the URL
in the row — so a snapshot never redraws an unchanged diagram, and the GUI thread never rasterises
anything. The cache lives in the OS temp directory because it is derived data: losing it costs a redraw,
and it keeps the database path the only OS-specific location in the codebase.

## View behaviour worth knowing

- **Ctrl+Z has to be handled in the delegate.** `TextEdit` swallows it for its own per-widget undo, which
  knows nothing about block creation, indentation, or other days, so the delegate's `Keys` handler claims
  it first, flushes any unsaved text, then calls the model's undo/redo. The window-level `Shortcut`s only
  cover the case where focus sits outside a block.
- **cxx-qt does not camelCase Rust identifiers for QML.** `submit_block` is `submit_block` in QML, not
  `submitBlock`. A mismatch compiles and fails only when the key is pressed, so a unit test scans the
  QML for `journal.<name>(` calls and asserts each is a declared `#[qinvokable]`.
- **The agenda is spliced in by the bridge, not by the column.** `apply` inserts the agenda rows after the
  first day's blocks when building the row list, which is what places them "right under today" without the
  list having to know what an agenda is. An empty agenda contributes no rows at all, so there is no heading
  to hide.
- **The agenda checkbox completes; the block checkbox cycles.** Checking something off means "done", so an
  agenda row calls `set_marker(row, "DONE")` rather than the cycle — otherwise one click would only move
  TODO to DOING and the row would stay.
- **A block's marker box is only drawn when it says something, or on hover/focus.** An always-visible empty
  box put a faint square beside every one of the fourteen empty days.
- **Layout is anchored, not a `Row`.** An editor whose `width` is computed from its own `x` inside a `Row`
  is a binding loop, and Qt resolves it by collapsing the width to 0 — which looks exactly like "clicking
  a block does nothing".
- **Qt 6.12 ships Controls' `Calendar` as a singleton**, so it cannot be instantiated at all
  ("Composite Singleton Type Calendar is not creatable"). The date picker is built from `MonthGrid` +
  `DayOfWeekRow` + `CalendarModel`, with a focusable content item so `Enter`/`Esc`/arrow keys work.
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
- **A block has two views of one text.** At rest it is a read-only rich `TextEdit` over the `html` role —
  the markdown rendered, marks gone — and the editable `TextEdit` is hidden. Clicking the rendered text
  enters edit mode at the caret the click landed on, translated back into the raw text by `raw_position`
  (the rendered text is shorter than the raw one, so the two offsets are not the same number). Moving the
  caret away renders the block again. `editingRow` lives on the window, not on a delegate, because every
  save rebuilds the delegates and the open block has to stay open across that.
- **An empty block is always the editor**, since there is nothing rendered to click.
- **Focus is taken in the same event, not on the next frame.** A keystroke typed the instant a block is
  clicked has to land in it, and a frame's delay silently eats it — this is what made starting a fence
  feel broken. `beginEdit` and the caret restore after a rebuild both call the focus path directly and
  fall back to a deferred attempt only when the row's delegate is not instantiated yet. A keystroke typed
  inside the round trip of a split — before the new block exists — is still dropped; at human typing speed
  the split has landed.
- **A save waits for an input method to finish composing.** While `inputMethodComposing` is true the
  debounced save is held back and restarted when it clears, because saving rebuilds the model, the rebuild
  replaces the editor holding the composition, and the composed character goes with it. This one is stated
  from the mechanism rather than from a test: Qt Quick ignores a synthesised input-method event, so the
  harness cannot exercise the path — a dead key on the Spanish layout, which is what ` is there, is the
  case it exists for.
- **Markdown is applied as text formats, never as content.** A `QSyntaxHighlighter` is attached to each
  block editor's document and rescans that document's own text on every change, so a link still being
  typed is already styled and nothing waits for the debounced save. Formats carry the resolved weight,
  size and colour — the palette lives in `markdown.rs`, and the shim knows nothing about markdown. It
  sets no characters, so the caret, the undo stack and the stored markdown behave exactly as they did
  before a block was styled. `highlightBlock` is handed one paragraph at a time while a fence spans
  lines, so the shim passes the whole document with the paragraph's range and Rust cuts the answer down to
  it.
- **Enter continues a fence; elsewhere it splits the block.** With the caret inside an open fence — on the
  line that opened it or in its body — Enter adds a line to the block, so typing ``` is all it takes to
  start a code block; on the closing line, or outside any fence, Enter splits into a new block as before.
  `Shift+Enter` still breaks a line anywhere, which is how the first line of a fence is started. `END` on a
  multi-line block stops at the end of the line, so anything that means "the end of the block" uses
  `Ctrl+End`.
- **A link in an editor needs Ctrl.** Blocks are always in edit mode, so a plain click has to keep meaning
  "put the caret here". Ctrl+click resolves the character under the pointer through `link_at`, which
  rescans the *editor's* text rather than the stored one, so an unsaved link is clickable too. One
  function, `openLink`, decides what a `page:` href or a URL does, for the editor and for read-only rows
  alike.
- **A read-only row is rendered, not styled.** The agenda and reference rows are `Label`s with rich text
  from the `html` role: the scan's dim spans are dropped, so the row reads as finished text, and its links
  navigate on a plain click.
- **The diagram is drawn under the fence, at its natural size.** A small graph stays small; only a wide
  one is scaled down, and a very tall one is capped. A fence that does not parse keeps its code and adds
  one muted line, produced only after the typing pause so half-typed diagrams do not flicker.

## Failure

`start()` sets `error` and leaves `ready` false when the store refuses to open; QML then shows the error
pane (message, path, Retry, Quit) and hides the column. `ready` is the only gate on rendering rows, so a
broken database can never look like an empty journal.

## Known limitations

- Markdown styling cannot invent glyphs: while a block is being edited, a list's `-` and a quote's `>` stay
  visible, dimmed, instead of becoming a drawn bullet or a bar — they do disappear in the rendered form.
  Folding is likewise not implemented: every block renders expanded, and link autocompletion when typing
  `[[` does not exist yet. A diagram cannot be enlarged, and it is drawn in blocks only — a summary row
  shows the fence as code, not as a picture.
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
a block, scrolling the column to its end to pull in an older chunk, Ctrl+Enter to mark a block,
Ctrl+Shift+S then Enter to schedule it, clicking the agenda checkbox to complete it, typing
`[[My Project]]` to create and link a page, opening that page from the switcher, expanding its unlinked
mentions, typing `**bold**` and `code` and reading back the *formats* the editor applied, leaving the block
to see it render and clicking it to see its marks come back, typing immediately after that click and again
after Enter to prove no keystroke is eaten, typing ``` in a fresh block to start a code block, building the
fence with plain Enter once it is open and checking the diagram it drew, a fence that does not parse and the
one line it reports, and finally a close with a pending debounce. Each step is checked against the
stored rows and depths, and every step saves a PNG so a human (or a reviewing agent) can see the rendered
window rather than trust a passing assertion.

Two harness lessons that cost a run each. `grabWindow()` races Qt's threaded render loop and can take the
whole process down inside `QRhi::beginFrame`; run the harness with `QSG_RENDER_LOOP=basic`. And Qt Quick
ignores synthetic wheel events under this compositor whatever device or phase they carry, so the "load an
older chunk" step drives the column to its end through the ListView rather than pretending to scroll it —
what is under test is the end-of-column path, not Qt's hit-testing.

`--livetest-error` covers the other half: it points the store at an unopenable path and asserts the error
pane appears with no rows, screenshotting it. That run is what caught the error heading being drawn in the
theme's default near-white on a cream background.

Qt key codes are a trap worth stating once: `0x01000007` is Delete, not End (`0x01000011`). A wrong
constant in the harness looks exactly like an application bug.

The `driver` feature is the boundary: without it, `src/driver.rs`, `driver_shim.cpp` and `livetest.rs` are
not compiled and the shipped binary carries no test hook. Besides the input helpers it exposes two
readers: the text or source of a named item (`named_text`, for a rendered block, a summary row and a
diagram's URL) and the formats applied to one character of an editor (`editor_format`). Its block helpers
address a row and then pick the view inside it, because a block may be showing either the editor or its
rendered form, and a row with a diagram is far taller than its text. Formatting has to be checked
that way because a highlighter writes to the block's *layout*, not to the document's character formats, and
a screenshot alone cannot tell a passing render from a lucky one.
