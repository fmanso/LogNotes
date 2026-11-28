# Search

Ctrl+K focuses the search field. Typing opens a grouped list of page and block hits. Enter opens the
first hit, Ctrl+Enter opens the full results view, and Escape returns to the journal column.

## Sub-features

- `search-open` focuses the search field with Ctrl+K.
- `search-hits` finds pages and blocks that mention the query.
- `search-page` opens the page named by the first hit with Enter.
- `search-block` opens a block hit with the caret on the match.
- `search-all` opens the full results view with Ctrl+Enter.
- `search-escape` leaves the results view and returns to the journal.

## How to get to it (user POV)

- Press Ctrl+K anywhere in the window.
- Type a page or block query. Page hits are listed before block hits.
- Press Enter on a hit, or Ctrl+Enter for the full results view.
- Press Escape to leave the results view.

## Driving it with livetest

Preconditions:

- `scripts/doctor.sh` passes.
- `scripts/verify.sh` has produced `livetest.log` and `shots/`.
- The page `My Project` and blocks that mention it exist from the earlier link steps.

- **Open the field.** Read `PASS ctrl+k opens the search field: focus=searchField`. Shot `25-search`.
- **Find hits.** Read `PASS typing finds the page and the blocks that mention it: hits=3`.
- **Open the page.** Read `PASS enter opens the page the first hit names`. Shot
  `26-search-opened-page`.
- **Open a block.** Read
  `PASS a block hit opens that block with the caret on the match`. Shot `27-search-opened-block`.
- **Full results.** Read `PASS ctrl+enter in the field opens the full results: view=search` and
  `PASS the results view lists headings and matches`. Shot `26a-all-results`.
- **Escape.** Read `PASS escape leaves the results and returns to the column: view=journal`.
- **Proof.** Keep shots `25-search` and `27-search-opened-block` with the log.

## Gotchas

- Results arrive after a 120 ms debounce. Read the `hits=` value after the list settles, not
  immediately after typing.
- The hit count includes both pages and blocks. A change in the seeded text changes `hits=3`.
- Ctrl+K while the search field already has focus does not clear it. Clear or replace the query.
- The full results view is a different view from the journal. Escape, not a click, returns.
- Opening a block hit moves the caret into that block. Expect the block editor to hold focus
  afterward.
