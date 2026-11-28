# Pages and links

Typing a `[[link]]` creates a page. A page shows its own blocks, the journal blocks that link it, and
unlinked mentions. Pages can be starred, renamed, deleted when unlinked, and reached by the sidebar,
the quick switcher, a link click, or the date control.

## Sub-features

- `link-complete` offers pages while typing `[[` and can create a new one.
- `page-open` opens a page from the quick switcher or from a link click.
- `page-references` shows journal blocks that link the page and an unlinked mentions group.
- `page-recent` lists the opened page under Recent.
- `page-star` stars a page into Favourites.
- `sidebar-toggle` hides and shows the sidebar with Ctrl+B.
- `page-menu` opens a page's header menu.
- `page-rename` rewrites links to the old name and undoes as one step.
- `page-delete` refuses a linked page and deletes an unlinked one.
- `block-move` moves a block to another page.
- `day-control` opens a day from the calendar and creates it on the first keystroke.
- `day-link` opens a day from a `[[YYYY-MM-DD]]` link.

## How to get to it (user POV)

- Type `[[` in a block to see page completion, then pick or finish a name.
- Press Ctrl+K, type a page name, and press Enter.
- Click the `My Project` link in a block at rest.
- Click a star on the page header to favourite it.
- Press Ctrl+B to hide or show the sidebar.
- Use the page header menu to rename, delete, or move a block.
- Click the date control to open the calendar, pick a day, and type in the unwritten day.
- Click a `[[YYYY-MM-DD]]` link in a block at rest.

## Driving it with livetest

Preconditions:

- `scripts/doctor.sh` passes.
- `scripts/verify.sh` has produced `livetest.log` and `shots/`.
- Earlier journal steps wrote `hello world! and My Project matters`, so the page exists by the time
  the page steps run.

- **Create from a link.** Shot `14-links-typed`. Read
  `PASS typing [[ offers the pages in the graph`, `PASS completion inserts a complete link`, and
  `PASS a new name commits and creates its page`. Shot `33b-link-complete`.
- **Open from the quick switcher.** Read
  `PASS the page created by the link can be opened from search` and
  `PASS the page shows blocks and references`. Shot `16-page-view`.
- **Recent.** Read `PASS the page just opened is listed as recent: sidebar={...}`.
- **Star and sidebar.** Read `PASS starring the page puts it in favourites`,
  `PASS ctrl+b hides the sidebar`, and `PASS and shows it again`. Shot `16a-starred`.
- **References and unlinked mentions.** Read `PASS the unlinked mentions group expands`,
  `PASS a summary row renders its markdown with a live page link`,
  `PASS the from-label goes to the block it names`, and
  `PASS and the caret lands in that block`. Shots `17-unlinked-expanded`, `17a-from-label`.
- **Journal return.** Read `PASS the Journal button returns to the column`.
- **Click a link at rest.** Read `PASS a link in a block at rest can be clicked`,
  `PASS clicking a link opens that page`, and `PASS the page a link opened takes a keystroke`. Shots
  `28-link-opened`, `29-link-typed`.
- **Page menu and rename.** Read `PASS a named page opens its own header menu`,
  `PASS a page rename keeps the page and its block`, and
  `PASS the journal link follows the rename`. Shots `29a-rename-dialog-light`,
  `29a-rename-dialog-dark`, `29a-page-renamed`.
- **Rename undo.** Read `PASS one undo restores the old page and the link text`.
- **Delete rules.** Read `PASS a linked page reports how many links it has`,
  `PASS a linked page is not deleted`, and
  `PASS an unlinked page is deleted and the journal returns`. Shot `29b-page-deleted`.
- **Move to page.** Read `PASS move to page sends the block to the chosen page`. Shots
  `29c-move-dialog-light`, `29c-move-dialog-dark`, `29c-moved-to-page`.
- **Day control.** Read `PASS the journal has a date control`,
  `PASS the date control closes without opening a day`,
  `PASS an unwritten day opens without a row`, and
  `PASS the first keystroke creates the day and its block`.
- **Day link.** Read
  `PASS clicking a date link opens that day as a page under its day heading`. Shot `33-day-page`.
- **Proof.** Keep shots `16-page-view`, `17-unlinked-expanded`, `29a-page-renamed`, and
  `33-day-page`.

## Gotchas

- Click a link only when the block is at rest. A click inside a focused editor moves the caret
  instead of following the link.
- Rename rewrites backlinks and is one undo. A partial rewrite means the link index is stale.
- A page with links refuses deletion. Unlink it first, or the delete step reports the link count.
- An unwritten day has no row. The heading appears, and the first keystroke creates the day.
- The quick switcher and the page view share the search field. Clear it before the next search step.
- The sidebar state is read as JSON. Quote the value when reporting Recent or Favourites.
