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

It also draws what the text means, without ever rewriting it: a block at rest and a read-only row are
rendered from escaped HTML, a ```mermaid fence is drawn to an SVG the view loads, and the editor itself is
plain text — styling is for reading, never for editing.

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

`rowCount`/`data` expose one flat list, rebuilt wholesale from a snapshot. A row is one of a few kinds,
carried in the `kind` role:

| kind | meaning |
| --- | --- |
| `heading` | A journal date. Carries `date`; the QML formats it for display. |
| `block` | A persisted block, carrying `blockId` and `depth`. |
| `placeholder` | The block an untouched day shows so the cursor has somewhere to land. `blockId` is `-1` and **nothing is written** until the first keystroke. |
| `agenda-header` | The `Scheduled` heading, present only when something is due. |
| `agenda-row` | A due block, spliced directly under today's blocks; carries the same `blockId`, so acting on it acts on the block. |
| `page-header` | The name of the page being viewed. Carries `date` when that page *is* a journal day — a date is a name a block can link to — so the header renders as a day and the star stays hidden. |
| `reference-header` / `reference-row` | `Linked References` and the blocks that link this page. |
| `unlinked-header` / `unlinked-row` | The collapsed `Unlinked mentions (N)` group and its rows. |

Roles are `kind`, `text`, `depth`, `date`, `blockId`, `placeholder`, `marker`, `scheduled`, `source`,
`overdue`, `html`, `diagram`, `diagramError`. `date` is a journal date, or the day a linked date named when
the row is a `page-header`; either way it stays ISO `YYYY-MM-DD` in the model and is formatted in QML — the
core never learns a locale, and the view never parses a date.

Three more properties belong to the shell rather than to a row: `theme` ("light" or "dark", from the
store's settings), `sidebar` (the two lists as `{"favourites": […], "recent": […]}`), and
`page_favourite` (whether the page in view is starred).

Two more belong to the quick switcher: `hits` is the last search answer as
`{"error": "…", "hits": […]}` (its own change signal is what rebuilds the list, so no counter is needed),
and `focus_row` is the row a search hit asked the caret to go to, -1 when none.

Two of those roles carry rendering rather than data: `html` is a read-only row's text already rendered
from the same scan the editor styles with (marks dropped, links carrying `page:` or a URL), and `diagram`
is the `file://` URL of the SVG drawn for that block's fence, with `diagramError` holding one line when
the fence did not parse.

Placeholder rows are why scrolling a year into the past cannot bloat the database: only a keystroke
materialises a day (`ensure_journal_page` + insert). This is a view-layer rule, not a schema one.

## Two views

The model shows either the journal column or one page — a named page, or a journal day reached by its date.
`open_page(name)` / `show_journal()` set the mode, which is carried to the worker with every job so each
snapshot is built for the view that asked for it: a page snapshot is its blocks plus `references(page)`, and
for a date that page is the journal day itself; a journal snapshot is days plus the agenda. So a day has two
views — where it sits in the column, and its own page — and only the column carries the agenda.

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
which is a single command so that pressing Enter is a single undo step, and `SetMarkerWithContent` /
`SetScheduledWithContent`, which exist for the same reason: a slash command's rewritten text and the state
it set are one command, so one `Ctrl+Z` takes the whole command back.

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
  the markdown rendered, marks gone — and the plain editor is hidden. Clicking the rendered text
  enters edit mode at the caret the click landed on, translated back into the raw text by `raw_position`
  (the rendered text is shorter than the raw one, so the two offsets are not the same number). Moving the
  caret away renders the block again. `editingRow` lives on the window, not on a delegate, because every
  save rebuilds the delegates and the open block has to stay open across that.
- **An empty block is always the editor**, since there is nothing rendered to click.
- **A click on a link at rest follows it.** The rendered block resolves the link with the view's own hit
  test (`linkAt`) before deciding to edit, because a `TapHandler` takes the press and the read-only view
  therefore never gets to emit `linkActivated`; the pointer turns into a hand over a link. `Ctrl+click` in
  the editor is unchanged.
- **Opening a page ensures it exists.** `open_page` dispatches `EnsurePage` before the snapshot, so a link
  naming a page nothing has written yet lands on an empty page you can type in. That needed a second fix:
  the placeholder row on a named page carries the page's *name* in its `date` field, and materialising it as
  a journal day failed — `Materialize` and `Submit` now carry a destination, day or page, chosen from the
  view's mode.
- **Ctrl+K is claimed in the delegate.** `TextEdit` binds it to delete-to-end-of-line, so the window
  shortcut alone would delete the block's text instead of opening the switcher whenever the caret is in a
  block. The delegate's key handler takes it first, and the window shortcut covers the rest.
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
- **Editing is plain text.** The editor is a bare `TextEdit`: no character formats, no syntax
  highlighter, nothing styled — every character the same weight, colour and size, marks showing as
  themselves. Styling belongs to the rendered view only, which is why nothing in the editor can fight the
  caret or the undo stack. This started as a highlighter that dimmed the marks while typing; it was
  removed, and with it the hand-written C++ shim and the format buffer that fed it.
- **Enter continues a fence; elsewhere it splits the block.** With the caret inside an open fence — on the
  line that opened it or in its body — Enter adds a line to the block, so typing ``` is all it takes to
  start a code block; on the closing line, or outside any fence, Enter splits into a new block as before.
  `Shift+Enter` still breaks a line anywhere, which is how the first line of a fence is started. `END` on a
  multi-line block stops at the end of the line, so anything that means "the end of the block" uses
  `Ctrl+End`.
- **A slash opens the block's commands.** Typing `/` where it begins a word — start of the block or after
a space, so `and/or` and a URL never trigger it — opens a menu beside the caret that filters as you type:
`TODO`/`DOING`/`DONE`/`Remove status`, `Scheduled`/`Clear date`, `Today`/`Tomorrow`/`Pick a date`, `Time`.
`Up`/`Down` move, `Enter` or a click runs one, `Esc` leaves the token as typed. Picking replaces the token
with the command's effect and never types markdown into the block, and each command is one undo step.

  Three things make it hold together. Its state lives on the window, not on a delegate, because every save
  rebuilds the delegates; the popup takes no focus (`focus: false`) so keystrokes keep arriving in the
  editor, whose key handler claims the navigation keys before their old meanings; and **saving is held for
  as long as it is open** — the debounce does not restart — so the editor, and the caret rectangle the menu
  hangs from, survives the menu's lifetime. Every exit writes what was typed: leaving the token resumes the
  debounce, a page switch or another block runs an immediate write, and a dismissed token stays as text.
  The token scan is in QML on purpose: `cursorPosition` and JS strings are both UTF-16, while core's offsets
  are bytes, so scanning there avoids the conversion the raw/rendered caret already has to fight.
- **A day page's header reads as a day.** A journal day opened by name (a `[[2025-11-02]]` link, or a date
  hit in the switcher) is just a page row, so the page branch of the snapshot serves it unchanged — but its
  header carries the day in the `date` role, which renders the column's own "Monday, November 2, 2026" and
  hides the favourite star. No agenda there: the agenda belongs to today's place in the column, and a day
  page shows the day's blocks and references only.
- **The date picker has two purposes, one write.** `Ctrl+Shift+S` opens it to set a block's scheduled date;
  the slash menu opens it to write the picked day into the text as a link, or (via `Scheduled`) to do the
  former while also eating the token. The second case keeps the pending token on the window and defers the
  text edit to *accept*, so the date and the rewritten text land as one command; `Cancel`/`Esc` writes the
  token as it was typed, and `Clear` clears the date through the same composite. The dialog is opened on the
  next turn (`Qt.callLater`) because a popup opened while the menu's overlay is still closing gets the focus
  but not the keys.
- **A link in an editor needs Ctrl.** Blocks are always in edit mode, so a plain click has to keep meaning
  "put the caret here". Ctrl+click resolves the character under the pointer through `link_at`, which
  rescans the *editor's* text rather than the stored one, so an unsaved link is clickable too. One
  function, `openLink`, decides what a `page:` href or a URL does, for the editor and for read-only rows
  alike.
- **One palette, keyed by theme.** Every colour in the window is a role on a `QtObject` (`page`, `panel`,
  `rule`, `border`, `text`, muted and faint text, `hover`, `accent`, `danger`, `success`, `warning`,
  `markerIdle`, `scrim`), computed from `root.darkTheme`. A third theme would be another table of roles,
  not a sweep through the file. The theme is read from the store, so the window fades in once the first
  snapshot arrives — `opacity`, not `visible`: a window hidden and then shown gets tiled by the
  compositor, which changes every layout assumption in the app.
- **The bar and the sidebar are the shell.** The bar holds the wordmark, the search field, a Journal
  button, a sidebar toggle and the theme toggle; typing in the field opens the results just under it, and
  `Ctrl+K` only puts the caret there. The sidebar lists Favourites then Recent (eight), and the Pages
  popup is gone — one way to reach a page, not two. A page is starred from the star in its own header.
- **The preview is a preview, and the field can hand over to a full view.** The results panel is an item
  anchored under the search field — not a `Popup`, whose position in the overlay is computed by hand and
  which put itself outside the window when the numbers disagreed — clipped and sized to the space below the
  bar. It lists five pages and twenty blocks; when there are more, its last row reads "All results (N)" and
  `Ctrl+Enter` opens the same thing from the field. That view is a *mode*, like a page: every match as column
  rows, Pages then Blocks, each block with what matched picked out behind it, and `Esc` returns to the
  journal.
- **A search term belongs to the hit that carried it.** It is captured when a hit is activated and
  consumed by the block that hit names; every other navigation forgets it. Left behind, it moved the
  caret to where that word happened to sit in a *later* block — which is how a reference click put the
  caret inside a link and renamed the page it pointed at.
- **Focus leaves a block only when something else takes it.** The blur that follows a rebuild cannot say
  whether the caret is gone or merely being handed back, so `rebuilding` stays up until the next turn
  (`Qt.callLater`) and the delegate that was destroyed reports its blur while the flag is still set.
  The search field hands focus back when a hit is activated — leaving it there meant the next keystroke
  landed in the search box instead of the page that had just opened.
- **`Ctrl+K` and `Ctrl+B` are handled in the delegate, not by a window shortcut.** A window shortcut runs
  before the focused item but does not consume the key, so a delegate that acted as well fired twice, and one
  that only consumed depended on the shortcut having been delivered — which it is not when the window is not
  active. The delegate does the work and takes the key; `TextEdit` would otherwise bind them to
  delete-to-end-of-line and bold. A window `Shortcut` runs before the
  focused item sees the key but does not consume it, and `TextEdit` binds the same keys to
  delete-to-end-of-line and bold — so a delegate that *acted* as well toggled twice, and one that did
  nothing let the editor eat the key. The delegate now accepts the key without acting: one action, and
  nothing for the editor to do with it.
- **The list keeps no off-screen delegates.** `cacheBuffer: 0` on the column, because every save rebuilds
  the model and a cached delegate from before the rebuild lingers in the tree — invisible on screen,
  but findable by anything that walks it, which is how the live test first clicked a stale star.
- **Search is one overlay over the whole graph.** `Ctrl+K` opens a modal with one field; typing asks core
  (120 ms after the last keystroke) and the answer lands in `hits`, which QML turns into a `ListModel` of
  page hits and then block hits, each with where it lives — `today`, a date, or a page name. `Up`/`Down`
  and `Enter` drive it, `Esc` closes it. Opening a block hit asks for its page or day and for the caret to
  go to `focus_row`; the caret is then put on the match by looking for the query in that row's text, since
  core returns where the block is, not where in the block the words are.
- **One button style, one popup palette.** `QuietButton` is the window's only button: an outlined control in
  the page's own colours. The Pages popup and the date picker are cream panels with a hairline border and a
  tinted hover row, because the control style's default chrome is dark and the text drawn on it was
  `#242424` on `#2a2a2a` — a page list you could not read.
- **Links carry the theme's accent, inline.** Qt draws an anchor in the palette's stock link blue, which
  belongs to neither theme — harsh on a dark page, foreign on a cream one. Every rendered anchor therefore
  carries `style="color:…"` from the same palette as the fences, and keeps its underline as the affordance.
- **Code fences are coloured by the view's palette.** The four colours live in `CODE_COLOURS` in the bridge
  and are handed to `to_html_block`; core decides what a token is, the view decides what it looks like. A
  fence whose language nobody knows renders as plain code.
- **A past day nobody wrote in leaves no trace.** `shows_day` keeps the newest day — that is where the
  cursor goes — and every day before it only when it has blocks. The column is a record of what was
  written, not of the calendar, and it fills itself: while it is shorter than the window it pulls older
  chunks (three at most, and before the window appears, because each pull resets the model and doing
  that after the caret is granted loses it).
- **A reference row's "from …" label goes to the block.** It opens the block's page or journal day and
  puts the caret in the block, through the same `open_block` path a search hit uses; a `[[link]]` inside
  the row still goes to *that* page. The target sits over the label rather than inside it, because a
  MouseArea nested in a Label is not reliably hit.
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
  shows the fence as code, not as a picture. The slash menu lists the commands the app has; code fences and
  markdown marks are typed, not inserted.
- Blocks cannot be reordered (only indented, outdented, created, edited, deleted).
- The focus restoration is index-based; a keypress that changes the row count between snapshot and
  rebuild can land the caret one row off.
- `cursorPosition` from QML is a UTF-16 offset while the core splits at UTF-8 byte boundaries; they agree
  for BMP text and diverge for characters outside it (emoji), where a split lands on the nearest boundary.

## Live testing

The window can be driven without a person at the keyboard, which is how its input handling gets checked:

```
make livetest      # build and drive the window, screenshots in $TMPDIR/lognotes-livetest
make test          # every unit test
make build         # the app, debug
make release       # the app, release
make run           # the app itself, on your display
```

The Makefile exists for two reasons: every build runs in its own cgroup slice with a memory cap, because
a cargo build inside the desktop's own scope once got the whole session killed by oomd; and with that cap
in place it also runs with six jobs, which took the driven run from about three and a half minutes to
one. `make run` is the exception — it is the app you are looking at, so it stays in your session.

It writes to a throwaway database under `$TMPDIR/lognotes-livetest`, loads the same `main.qml` the app
loads, and drives it through the real input path: typing, Enter, Tab, Shift+Tab, Ctrl+Z, a mouse click on
a block, scrolling the column to its end to pull in an older chunk, Ctrl+Enter to mark a block,
Ctrl+Shift+S then Enter to schedule it, clicking the agenda checkbox to complete it, typing
`[[My Project]]` to create and link a page, opening that page from the switcher, expanding its unlinked
mentions, typing `**bold**` and `code` and reading back the *formats* the editor applied, leaving the block
to see it render and clicking it to see its marks come back, typing immediately after that click and again
after Enter to prove no keystroke is eaten, typing ``` in a fresh block to start a code block, building the
fence with plain Enter once it is open and checking the diagram it drew, a fence that does not parse and the
one line it reports, opening the quick switcher with Ctrl+K to find a page and then a block, a python fence read back as
coloured HTML, starring a page and reading the sidebar back, hiding the sidebar with Ctrl+B, switching
the theme to dark and back, clicking a link in a block at rest and typing on the page it opened, typing `/`
to open the command menu and Enter to run what it highlights, narrowing it with another letter and reaching
the next command with Down, Esc leaving the token as typed, `/scheduled` through the calendar and one
Ctrl+Z taking its date and its text back together, and clicking the date link it wrote to open that day as a
page — then finally a close with a pending debounce. Each step is checked against the
stored rows and depths, and every step saves a PNG so a human (or a reviewing agent) can see the rendered
window rather than trust a passing assertion.

The slash-command steps assume no row on purpose: a synthetic click lands unreliably under this compositor
(a dozen earlier steps already fail that way on the base commit), so each one reads the store once and finds
its block by content — "the block that holds today's link" — rather than by an index it cannot trust.

Three habits keep the run honest, after a stretch of steps that passed on one run and failed on the next:

- **`settle()` instead of sleeping.** It waits for the store to answer, then outlasts the 250 ms debounce,
  then waits for the model to stop changing. A fixed sleep either read mid-rebuild — when the delegates
  have been destroyed and not rebuilt — or was too short for the store to open at launch.
- **Find a row by its content, never by its index.** `row_with("**bold**")` looks for the block; which row
  sits at which index is the view's business and moves as rows come and go. A rendered row still reports
  its raw text, so this works whether the block is being edited or not.
- **Aim a click that is only meant to move the caret.** `click_block_at(row, fraction)` exists because a
  click at 12% of a row still landed inside a `[[link]]` and navigated — the feature working as designed,
  on a test that did not mean it.

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
readers: the text or source of a named item (`named_text`, for a summary row and a diagram's URL), whether
the window is actually shown (`window_visible` — `grabWindow()` happily captures a hidden window, so the
first check of the run asserts it), the
HTML a block row renders (`row_html`), any string property of the model (`journal_property` — the theme,
the sidebar, the star), whether a named item is on screen (`visible`), and how many rows the quick
switcher is showing (`search_count`). It can
also click a real link inside a block at rest (`click_link`), found with the view's own hit test rather than
guessed at. Its block helpers address a
row and then pick the view inside it, because a block may be showing either the editor or its rendered form,
and a row with a diagram is far taller than its text. It activates its window on attach, because an input
method only delivers to an active one.
