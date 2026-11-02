---
id: lognotes
type: goal-and-requirements
status: active
title: LogNotes — a personal Logseq 0.x clone
tags: [personal-tool, notes, outliner, desktop]
---

# LogNotes

A personal, trimmed-down rebuild of the Logseq 0.x workflow: journal-first outliner, page links and
backlinks, and scheduled tasks that surface on Today.

## Overview

A daily-use notes tool for one person — its maker — that keeps the Logseq 0.x habits and drops the rest
of the product. Every note is a block in a nested outliner on a journal or page; blocks link to pages
with `[[brackets]]` and `#tags`, pages collect their references, and tasks carry a scheduled date so
they appear on the Today view when they come due. Notes live in a local database rather than a folder of
Markdown files, and the app runs as a desktop application on Windows and Linux.

It replaces the maker's use of Logseq 0.x, and stands in front of Logseq 2.x — whose changes are the
reason for building it.

## Problem

The maker used Logseq 0.x daily and did not follow it to 2.x: the changes introduced there are ones the
maker does not want, while most of what Logseq offers — queries, properties, graph view, whiteboards,
flashcards, plugins — goes unused. The result is a tool that is simultaneously wrong in the parts that
matter and heavy in the parts that do not. The maker wants the 0.x working set exactly, in an
application small enough to fully own and trim.

## Capabilities

- **Journal-first outliner.** A journal page per day; notes are nested blocks typed and edited in
  place, indented and outdented, all keyboard-driven.
- **Pages, links, tags, backlinks.** `[[page]]` links and `#tags` create and link pages; each page lists
  its linked references at the bottom, both explicit links and plain mentions, as 0.x does. *(Plain-mention
  references inferred from 0.x behavior — unconfirmed.)*
- **Markdown, rendered.** A block shows its text finished: bold, italic, code, headings and quotes are
  styled, `[[links]]`, `#tags` and external links are followable, a ```mermaid fence draws its diagram
  beneath itself, and a code fence is coloured by its language. Clicking a block swaps in its plain text — every character alike, marks and all, nothing
  styled — to edit it, and leaving it renders again. What is stored is always the markdown. The agenda and
  the reference panels show the same finished rendering.
- **Tasks.** Blocks are marked `TODO`, `DOING`, or `DONE`, cycled by a shortcut. *(`DOING` inferred from
  0.x task markers — unconfirmed; `DONE` is confirmed.)*
- **Scheduled blocks.** A block can carry a `SCHEDULED` date — the maker's way of remembering future
  work.
- **Today's page shows the agenda.** Today's journal page lists the blocks scheduled within the next 7
  days right below the day's own blocks, with the linked references at the bottom — the 0.x journal
  layout. An item stays listed until it is marked `DONE`, and disappears immediately once it is. *(The
  7-day horizon reads "like a week before hand" — unconfirmed as an exact number. That overdue items stay
  listed is inferred from "if marked as DONE it should disappear" — unconfirmed.)*
- **A link goes where it points.** A click on `[[page]]` or `#tag` in a block opens that page, creating it if
  nothing has written it yet, so the click always lands somewhere you can type.
- **A date names its day.** `[[2025-11-02]]` is a link to that journal day, not a look-alike page beside the
  calendar: clicking it opens the day, and the quick switcher finds written days by date. A day opened that
  way shows as a page — its blocks and their linked references — while the agenda stays on today's place in
  the column.
- **Everything a block can do, from the block.** Typing `/` opens a command menu beside the caret: set the
  task marker, clear it, schedule the block through the calendar, clear that date, write a link to today,
  tomorrow or a day you pick, or insert the current time. Typing narrows the list, `Enter` or a click runs
  one, `Esc` leaves the text as typed — and a command changes the block, never the words in it, so `/todo`
  leaves a task rather than the letters. Each one is a single undo.
- **A shell you can work from.** A bar with a search field, a way back to the journal, and a light/dark theme
  switch that is remembered; a sidebar of the pages you starred and the ones you opened most recently.
- **Search.** `Ctrl+K` opens a quick switcher over the whole graph: page names and written journal days
  first, then blocks whose text matches, as you type. `Enter` opens the page or day, or puts the caret on the
  match in the block it found.
- **Your notes are not locked in.** A Settings page says where the database and its backups live and opens
  either folder in the file manager; a backup is written once a day as the notes open (the newest seven
  kept) and on demand; export writes every page and day as Markdown, in the folders Logseq 0.x reads; and
  import reads such a folder back in, so a graph can leave this app and come back.
- **Local database.** All notes are stored locally in a database that is the single source of truth; the
  app works fully offline with no account.

## Out of scope (by decision)

Not built: queries and block properties, graph view, whiteboards, flashcards, plugins and themes, syncing
between devices, and publishing. *(Read as the maker's non-selection of these features — inferred,
unconfirmed as hard exclusions.)*

## Alternatives Considered

| Tool | Gap for this use case | URL |
| --- | --- | --- |
| Logseq OG | The file-based 0.x line, still developed under its own name — it covers the whole workflow, which is the problem: every unused feature (queries, properties, graph, whiteboards, flashcards, plugins) ships too. Also file-canonical, where this app keeps a database. | https://github.com/logseq/og |
| Obsidian | Document-first, not block-first: notes are pages of text with daily notes bolted on, so there is no nested outliner and no scheduled-block agenda without plugins. | https://obsidian.md |
| Anytype | Local-first and encrypted, but object-based rather than an outliner: no journal-first block editing and no `SCHEDULED`-on-a-block workflow. | https://anytype.io |
| Tana | Closest in feel (day-centric outliner, supertags), but cloud-first with a required account — the app here is offline and local. | https://tana.inc |

## Tech Notes

Desktop app for **Windows and Linux**, built as **Rust** with a **Qt 6** UI via **cxx-qt** (QML for the
interface, Rust for the model and storage) and **SQLite** as the local store.

Two decisions worth keeping in view:

- **The database is canonical, not Markdown files.** Chosen deliberately after the trade-off was raised:
  it is what the maker asked for, and the consequence accepted is that notes are not readable or
  portable as plain files without the app. Providing an export path is left open, not required for the
  first build.
- **No importer in the first build.** The app starts with an empty graph and Logseq 0.x stays in use in
  parallel until the clone is trusted.
