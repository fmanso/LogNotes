# Journal writing

The journal is the default view. The caret lands in today's block, and typing, Enter, indentation,
reordering, undo, fences, and the slash menu all operate on ordinary blocks that persist to SQLite.

## Sub-features

- `journal-open` opens the window with the caret in today's block.
- `journal-type` sends text to the store.
- `journal-split` splits a block on Enter and keeps the caret in the new block.
- `journal-reorder` swaps a block with a sibling by Alt+Up/Down and Ctrl+Shift+Up/Down.
- `journal-navigate` moves the caret between blocks at the first and last line.
- `journal-indent` indents with Tab, outdents with Shift+Tab, and folds a parent with its chevron.
- `journal-undo` undoes and redoes edits with Ctrl+Z and Ctrl+Y.
- `journal-fence` keeps a backtick fence in one block and renders Mermaid and Python fences.
- `journal-slash` opens the command menu with `/`, filters it, runs a command, and leaves the token
  on Escape.

## How to get to it (user POV)

- Launch the app. The caret is in today's block.
- Type text, press Enter, Tab, Shift+Tab, Alt+Up, Alt+Down, Ctrl+Shift+Up, Ctrl+Shift+Down, Ctrl+Z,
  and Ctrl+Y in the block editor.
- Click a block that is at rest to edit it. Click the chevron to fold its children.
- Type `` ``` `` in a block to start a fence.
- Type `/` at the start of a block to open the command menu, then type to filter it.

## Driving it with livetest

Preconditions:

- `scripts/doctor.sh` passes.
- `scripts/verify.sh` has produced `livetest.log` and `shots/`.
- The labels below come from the run, in scenario order.

- **Open.** The first shot is `01-open`. Read `PASS the window is on screen` and
  `PASS caret is in today's block on launch`.
- **Type and split.** Read `PASS typing reached the store: blocks=["hello world"]`, then
  `PASS enter split the block and left an empty one`. Shots `02-typed`, `03-enter`, `04-second-block`.
- **Reorder.** Read the three labels starting `alt+up swaps`, `ctrl+shift+down swaps`, and
  `the navigation edits undo back`.
- **Navigate.** Read `up from the first line moves to the previous block` and
  `down from the last line moves to the next block`.
- **Indent and fold.** Read `tab indented the block`, `folding hides a block's children`,
  `unfolding brings them back`, and `shift+tab outdented it`. Shots `05-indent`, `06-outdent`.
- **Undo.** Read `ctrl+z undid the outdent`. Shot `07-undo`.
- **Click to edit.** Read `clicking a block edits that block`, then the two labels beginning
  `typing straight after clicking` and `typing straight after Enter`. Shot `20-click-to-edit`.
- **Fences.** Read ``typing ``` in a new block keeps the caret in it``,
  `enter inside a fence kept the whole of it in one block`,
  `the mermaid fence drew a diagram next to itself`,
  `a fence that does not parse says so in one quiet line`, and
  `a python fence renders its keywords and numbers coloured`. Shots `21-typing-backticks`,
  `22-diagram`, `23-diagram-error`, `24-python-fence`.
- **Slash menu.** Read `typing a slash opens the command menu`,
  `a slash command marks the block and eats the token it replaced`,
  `the menu follows what is typed after the slash`,
  `down then enter picks the next match, and a date command writes a link to the day`, and
  `esc leaves the token as typed instead of running a command`. Shots `30-slash-menu`,
  `31-slash-todo`, `32-slash-filtered`.
- **Proof.** Keep shots `03-enter`, `05-indent`, `22-diagram`, and `31-slash-todo` with the log.

## Gotchas

- The slash filter steps are timing-sensitive. A lone failure there, with `/t` left in the block text
  and a query left in the search field, points at a focus race. Re-run before diagnosing a menu bug.
- `Alt+Up` and `Ctrl+Shift+Up` both reorder. The scenario exercises both, so a failure in one does
  not mean the shortcut is gone.
- A block being edited shows its plain text. A block at rest shows rendered Markdown. Check
  `rendered_html` for the rendered form and `editor_text` for the source.
- The fresh database makes ids and dates run-specific. Do not compare them across runs.
- The run wipes `${TMPDIR:-/tmp}/lognotes-livetest` at the start. Read the archived copy in the
  `verify.sh` output directory, not the temp directory.
