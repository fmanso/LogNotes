---
id: lognotes-core
type: module-design
status: active
title: lognotes-core — model, store, and queries
parent: architecture
tags: [rust, sqlite, storage]
---

# lognotes-core

Owns the note model and everything derived from it — pages, blocks, sibling order, link extraction, the
agenda rule — behind one small command-and-query surface. It is the only crate that speaks SQL.

## Responsibility

Turn commands into durable state and answer queries from it. It knows nothing about widgets, threads it
did not create, or how a block is drawn. What it guarantees is that the database can never be left in a
state the invariants in `architecture` forbid.

It also reads block text: one markdown scanner answers what a block links to, how it should look, and
which fences a diagram comes from, so the link table and the rendering cannot disagree.

## Boundary

- **Allowed dependencies:** `rusqlite` (with bundled SQLite), the standard library, and small utility
  crates for dates and ordering keys. Nothing from the workspace — this crate is a leaf.
- **Forbidden:** Qt or cxx-qt types, any dependency on `lognotes-qt`, async runtimes, network clients,
  global mutable state.
- **Depended on by:** `lognotes-qt`, which calls in and never the reverse. Keeping the arrow one-way is
  what lets the model be tested with `cargo test` alone.

`Store` is single-threaded by contract: created, used, and dropped on one thread, and not `Sync`. The
bridge owns the worker thread and is responsible for that guarantee.

## Schema

```sql
CREATE TABLE page (
  id           INTEGER PRIMARY KEY,
  name         TEXT NOT NULL,               -- as the user typed it
  name_key     TEXT NOT NULL UNIQUE,        -- lowercased + trimmed; identity for links
  kind         TEXT NOT NULL CHECK (kind IN ('page', 'journal')),
  journal_date TEXT,                        -- 'YYYY-MM-DD' for journals, NULL for pages
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL,
  CHECK ((kind = 'journal') = (journal_date IS NOT NULL))
);
CREATE UNIQUE INDEX page_journal_date ON page(journal_date) WHERE journal_date IS NOT NULL;

CREATE TABLE block (
  id             INTEGER PRIMARY KEY,
  page_id        INTEGER NOT NULL REFERENCES page(id) ON DELETE CASCADE,
  parent_id      INTEGER REFERENCES block(id) ON DELETE CASCADE,  -- NULL = top level of the page
  order_key      TEXT NOT NULL,             -- fractional key, unique among siblings
  content        TEXT NOT NULL DEFAULT '',
  marker         TEXT CHECK (marker IN ('TODO', 'DOING', 'DONE')),  -- NULL = plain block
  scheduled_date TEXT,                      -- 'YYYY-MM-DD' or NULL
  collapsed      INTEGER NOT NULL DEFAULT 0,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);
CREATE UNIQUE INDEX block_sibling_order ON block (page_id, IFNULL(parent_id, 0), order_key);
CREATE INDEX block_page  ON block (page_id);
CREATE INDEX block_agenda ON block (scheduled_date)
  WHERE scheduled_date IS NOT NULL AND marker IS NOT NULL;

CREATE TABLE block_link (
  block_id INTEGER NOT NULL REFERENCES block(id) ON DELETE CASCADE,
  page_id  INTEGER NOT NULL REFERENCES page(id) ON DELETE CASCADE,
  kind     TEXT NOT NULL CHECK (kind IN ('link', 'tag')),
  PRIMARY KEY (block_id, page_id)
);
CREATE INDEX block_link_page ON block_link (page_id);

CREATE VIRTUAL TABLE block_fts USING fts5 (content, content = 'block', content_rowid = 'id');
```

Two conventions the schema encodes deliberately:

- **Dates are text in `YYYY-MM-DD`.** Lexicographic order is calendar order, so range predicates stay
  plain string comparisons and no timezone ever enters the store. `scheduled_date` is independent of
  `marker` — a scheduled block that is not a task is legal, and the agenda rule decides what to list.
- **`block_fts` is an external-content FTS5 table over `block.content`**, kept current by the three
  triggers in schema v2 (`insert`/`delete`/`update OF content`) plus a one-off rebuild. This reverses this
  document's earlier note that the store would maintain it by hand: for external-content FTS5 the special
  `'delete'` command needs the *old* text, which only the trigger has. A v1 database upgrades in place.

## Commands

Every command validates, applies in one transaction, refreshes the derived rows it touches
(`block_link`, `block_fts`), and returns the snapshot its caller should render. Nothing half-applied is
ever visible.

| Command | Effect |
| --- | --- |
| `create_page(name)` | Creates a page, or returns the existing one whose `name_key` matches. |
| `ensure_page(name)` | Find-or-create a named page — what extraction calls when `[[Name]]` names something new. Empty names are refused with `BadPageName`. |
| `journal_page(date)` | **Lookup only**: the journal page for that date, or none. Reading never writes — that is what lets the column scroll past a year of empty days without creating them. |
| `ensure_journal_page(date)` | Finds or creates the journal page for that date. Called only when something is actually written to the day. |
| `insert_block(page, after, content)` | New block as the next sibling of `after` (or last child), with a key between its neighbours. |
| `update_content(block, content)` | Re-extracts links and reindexes text. |
| `split_block(block, offset)` | One command for Enter mid-block: keeps the text before `offset`, moves the rest into a new sibling. Splitting and inserting separately would show up as two undo steps. |
| `cycle_marker(block)` | Walks the marker cycle none → TODO → DOING → DONE → none. It lives here, not in the view, so a checkbox, a shortcut and the agenda cannot disagree about the order. |
| `set_marker(block, marker)` | Sets or clears the marker directly — what the agenda's checkbox uses to mean "done", as opposed to "next marker along". |
| `set_scheduled(block, date)` | Sets or clears the scheduled date. Scheduling a block that is not a task also marks it TODO, in the same composite action, because the agenda lists tasks and a date on an untasked block would silently never appear. |
| `indent(block)` / `outdent(block)` | Re-parents within the same page, keeping a valid key among the new siblings. |
| `move(block, page, after)` | Moves a block (with its subtree) between pages or positions. |
| `delete_block(block)` | Removes the block and its subtree. |
| `set_collapsed(block, bool)` | Persists fold state. |
| `undo()` / `redo()` | Apply the recorded inverse (or replay it) and return whether anything happened. |

Page creation is deliberately **outside** the history: undoing the first block of a day leaves an empty
page behind rather than deleting it. A page is an address, and an address that disappears under the
cursor is worse than one that lingers.

## Queries

| Query | Returns |
| --- | --- |
| `page_by_name(name_key)` | Page row, or none. |
| `page_blocks(page)` | Flattened rows: `id`, `depth`, `content`, `marker`, `scheduled_date`, `collapsed` — pre-order, honouring the order key. A collapsed block is included with its children omitted. |
| `pages()` | Every `kind = 'page'` page, ordered by `name_key` — the page switcher's list. Journal days are excluded: they are reached by scrolling, not by name. |
| `references(page)` | `ReferenceRow`s: blocks that link this page (`ReferenceKind::Linked`, from `block_link`) followed by plain mentions (`Mention`, from `block_fts`, minus the blocks that already link, matched on whole words). Each row carries the block's own page name and journal date. |
| `day(date)` | `JournalDay`: the day's blocks, or an empty block list with no page id when the day was never written. |
| `journal_days_ending_at(date, count)` | `count` dates ending at `date`, newest first — the chunk the column loads. |
| `agenda(today, horizon_days = 7)` | `AgendaRow`s: blocks with a marker in (`TODO`, `DOING`) and a `scheduled_date` up to `today + horizon`, **including past dates**, ordered by `scheduled_date` then page then order key. Each row carries the block, its owning page's name and journal date, and an `overdue` flag computed against `today` — so no caller has to compare dates itself. |
| `references(page)` | Explicit references from `block_link`, plus plain mentions from `block_fts` matched on `name_key` as whole words and excluding blocks already listed. |

The agenda rule and its unconfirmed parts are recorded in `lognotes`; this module implements them and
does not redefine them. The same holds for the invariant that a flat, depth-carrying row list is the shape
both `page_blocks` and `agenda` return — it is what lets one QML model render every view.

## Derived link state

Extraction runs inside every write that touches block text (`insert_block`, `update_content`, `split_block`,
and the restore path of a delete's inverse), in the same transaction, so `block_link` can never lag the text
it came from. `[[Page Name]]` and `#tag` are recognised; spans inside inline code are skipped; aliases and
`#[[multi word tags]]` are not supported.

Two consequences worth stating: extraction *creates* the pages it names (a link is an assertion the page
exists), while a plain mention matches only pages that already exist — prose never conjures a page. Pages
created by extraction stay behind if the link is later removed, like journal pages created by typing.

## Ordering keys

Sibling order is a fractional key: a sortable string strictly between two neighbours, no renumbering on
insert. Keys are generated in exactly one place in this crate, and the unique sibling index is the
backstop against a duplicate. Repeated insertion into the same gap lengthens keys; at personal-note scale
that is invisible, and a rebalance is therefore not implemented.

## Link extraction

On every content write, the store extracts:

- `[[Page Name]]` — any text except brackets; creates the page if missing (`kind = 'page'`).
- `#tag` — a single token with no whitespace; creates the page if missing.

Spans inside inline code (`` `…` ``) and fenced code are skipped — a code sample must not create a page.
Link aliases and `#[[multi word tags]]` are not supported in the first build. Extraction replaces that
block's derived rows wholesale, so deleting a link removes it.

The scanning itself is `markdown::scan` below; extraction keeps only the page-naming links and drops
markdown links, which point outside the graph.

## Markdown

`markdown` is the crate's one reading of block text, and everything that needs to know what a block means
asks it: `scan` returns styled spans, the links and tags with the byte ranges they occupy, and the fences
with their language and code. Spans are style *flags* — dim, bold, italic, strike, code, heading, link —
so nesting composes (italic inside bold) and a caller picks what to do with each.

Two rules make the rest of the app possible: syntax marks are their own dim spans, so a rendered view can
drop them while the raw text keeps them; and `to_html` / `to_html_block`
render a block as escaped HTML with the marks gone and links carrying `page:<name>` or their URL — one
line for a summary row, line breaks kept for a block at rest, where a fence has to stay a block of code.
Every character of the note is escaped there, so nothing a note says can become markup.

Three queries serve the editor's own behaviour: `in_fence` says whether a caret sits inside a fence (on the
line that opened it or in its body, not on the line that closes it), which is what makes Enter continue a
code block instead of starting a new one; `raw_position` translates a caret counted in the *rendered* text
back to the raw one, because a block at rest is clicked in text that has no marks in it; and `link_at`
finds the link under a click.

Offsets are bytes, as Rust slices are. UTF-16 positions — what a Qt text document counts in — are the
bridge's business, not this crate's.

## Migrations

`PRAGMA user_version` is the schema version. On open, steps run in order, each in its own transaction. A
file whose version is **newer** than the binary supports is refused with an error rather than opened —
better a clear refusal than a silently damaged graph. Opened files run `PRAGMA foreign_keys = ON` and WAL.

## Errors

One typed error enum — `NotFound`, `InvalidPosition` (no key fits between the neighbours), `BadOffset`
(not on a character boundary), `BadDate`, `BadPageName`, `SchemaTooNew`, `Storage` (wrapped sqlite error). No panic on any
input the bridge can pass. Every variant carries enough context to name the offending page or block in a
UI message.

Dates are validated by round-trip, not by `parse` alone: chrono accepts `2026-4-3`, and a store that
silently treats it as a second spelling of `2026-04-03` would hand out two keys for one day.

## Testing

`cargo test` against an in-memory database, no Qt and no fixtures on disk — which is only possible because
this crate has no Qt dependency.

Covered today: sibling order holds through insert, indent, and outdent; a subtree delete restores exactly
once with one undo, including its order keys; `split_block` is one history entry and refuses an offset
inside a character; undo crosses days in the order the edits were made; a no-op command records no history;
reading a day never creates it; journal pages are unique per date; malformed dates are refused.

Also covered from the task capability: the marker cycle in order; agenda boundaries at `today`, `today + 7`
and `today + 8`; overdue rows flagged and sorted first; a `DONE` block leaving the agenda; scheduling an
untasked block marking it TODO; clearing a date keeping the marker; and one undo removing both the date and
the auto-marker.

Also covered: `[[links]]` and `#tags` creating pages; a link becoming a linked reference and disappearing
when the link is removed; a plain mention appearing once, never duplicating a linked block, and matching
whole words only; mentions not creating pages; `pages()` listing named pages but not journals; and markers
and scheduled dates surviving a reindex.

Still owed: restoring a moved subtree across pages keeping `block_link` and `block_fts` consistent (block
reordering is not a capability yet).

Markdown is covered separately in `markdown`'s own tests: each syntax's spans and their dim marks, nested
emphasis, an unclosed marker staying plain, `snake_case` not italicising, links and tags with their
ranges, a tag stopping at punctuation, a fence collecting its lines and hiding the links inside them, a
fence still being typed, and every span landing on a character boundary. `to_html` is covered for
escaping (a note containing markup shows that markup), dropped marks, one line per block, and
`page:`/URL hrefs; `to_html_block` for keeping a fence's lines. `link_at` resolves a click to the link
under it, `in_fence` covers both edges of a fence and one that was never closed, and `raw_position` maps a
rendered caret back past the marks, in UTF-16 units and across an emoji.
