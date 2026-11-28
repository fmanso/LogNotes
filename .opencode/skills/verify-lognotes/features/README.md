# LogNotes verification map

This directory is the maintained source for proving user-facing behavior in LogNotes. Read this
index, run the harness once, then use the matching feature file to read the evidence.

## Baseline preconditions

- A Linux desktop session with `DISPLAY` or `WAYLAND_DISPLAY`, Rust stable, and Qt 6.8 or newer.
- The `driver` feature is declared in `lognotes-qt/Cargo.toml`.
- `scripts/doctor.sh` reports no `FAIL`.
- No other livetest is running. Two runs share and wipe `${TMPDIR:-/tmp}/lognotes-livetest`.
- The user's real graph at `${XDG_DATA_HOME:-$HOME/.local/share}/lognotes/` is never touched.

## Driving conventions

- Run `scripts/verify.sh` once from any directory. It finds the repo root.
- The scenario is one fixed script. There is no filter, so one run covers every feature file.
- Every run starts from a fresh database and a fresh screenshot directory.
- Cite the `PASS` or `FAIL` label and the shot file name, not just `all steps passed`.
- Treat labels as literal. Do not reword them in a report.
- The scenario continues after a failed check and stops on a panic. A partial log is still evidence
  for the steps it reached.

## Proof and skip reporting

- Prove the user path from the real window, not an internal setter.
- Quote the observed value from the label detail, for example `depths=[0, 1]`.
- Open the screenshot that matches the step. The shot line gives its archived path.
- A skipped entry point is reported as skipped, never as verified through another path.
- Re-run after a code change. Earlier artifacts are stale.
- Report an unreachable step with the label, the observed detail, and the shot.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the user-visible behavior.
It then uses exactly four H2 sections in this order.

1. `Sub-features` lists short IDs with one line for each behavior.
2. `How to get to it (user POV)` lists every user entry point.
3. `Driving it with livetest` starts with `Preconditions:` and pairs each user action with the exact
   label and shot to read.
4. `Gotchas` lists traps that waste or invalidate a run.

## Features

- [Journal writing](./journal-writing.md) covers typing, block splits, indentation, reordering,
  undo, fences, diagrams, and the slash menu.
- [Tasks and agenda](./tasks-and-agenda.md) covers TODO and DONE markers, scheduling, the calendar,
  and the agenda section.
- [Pages and links](./pages-and-links.md) covers link creation, page views, references, stars,
  rename, delete, move, and day navigation.
- [Search](./search.md) covers the quick switcher, grouped hits, opening a page or block, the full
  results view, and escape.
- [Themes and dialogs](./themes-and-dialogs.md) covers the light and dark switch and the themed
  app-owned dialogs.
- [Settings and portability](./settings-and-portability.md) covers settings, the note font, backup,
  restore, export, and import.
- [Agent bridge](./agent-bridge.md) covers the loopback control channel, project context, the
  digest and task write, and its single undo.
