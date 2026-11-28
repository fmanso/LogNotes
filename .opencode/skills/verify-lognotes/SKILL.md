---
name: verify-lognotes
description: Drive the real LogNotes desktop window to prove a change works. Use when verifying a feature, a UI or theme change, a storage change, or a regression in this repo. Runs the Qt livetest harness, captures screenshots and a PASS/FAIL log, and checks SQLite side effects beside what the screen shows.
---

# Verify LogNotes

LogNotes is a Qt 6 desktop app. The only honest proof of behavior is the real window, driven
through Qt's own event path. The repo ships that harness as `lognotes-qt/src/livetest.rs`, reached
with `make livetest`. This skill wraps that harness so a run leaves a log and screenshots an agent
can cite.

Read [`features/README.md`](features/README.md) before driving. It lists the mapped features and
the conventions the recipes assume.

## Launch

Requirements are Rust stable, Qt 6.8 or newer, and a display. The harness is a short-lived process,
not a server. It builds the app, opens the real `main.qml`, and exits when the scenario ends.

```bash
make livetest
```

The run takes about two to three minutes. It deletes `${TMPDIR:-/tmp}/lognotes-livetest` at the
start, then creates a fresh `lognotes.db` and `shots/` there. It never reads the user's real graph,
which lives at `${XDG_DATA_HOME:-$HOME/.local/share}/lognotes/lognotes.db`.

There is no readiness banner. The first check is `PASS the window is on screen`. A panic before
that line means the QML window failed to load.

If `systemd-run` is unavailable, call the binary directly. This is the command `make livetest`
expands to.

```bash
QSG_RENDER_LOOP=basic CARGO_BUILD_JOBS=6 cargo run -p lognotes-qt --features driver -- --livetest
```

`QSG_RENDER_LOOP=basic` is not optional. Without it, `grabWindow` can crash when the harness takes a
screenshot.

To poke at the app by hand instead, run `make run`. That instance uses the real data directory and
takes the single-writer lock. It does not interfere with a harness run.

## Doctor

Run the doctor first whenever a run fails in a way the log does not explain.

```bash
.opencode/skills/verify-lognotes/scripts/doctor.sh
```

It is read-only. It checks the repo root, `cargo` and `make`, the display, the `driver` feature,
whether a systemd user scope works, whether another livetest is already running, and whether the
debug binary resolves its libraries. Every line starts with `ok` or `FAIL`, and any `FAIL` makes
the script exit nonzero.

The concurrent-run check matters. Two livetests share `${TMPDIR:-/tmp}/lognotes-livetest` and delete
each other's working directory. A normal LogNotes instance is safe, because the database lock keeps
it out of the harness's temp graph.

## Drive

```bash
.opencode/skills/verify-lognotes/scripts/verify.sh [output-dir]
```

The script runs the whole scenario from the repo root, tees the log to `livetest.log`, copies every
screenshot out, and writes `summary.txt`. The default output is
`${TMPDIR:-/tmp}/lognotes-verify/<timestamp>`. It exits nonzero when the harness fails.

The scenario is one fixed script. There is no per-feature filter and no scenario-name argument.
Every feature file names the step labels that cover it, so a run of the whole script verifies every
mapped feature at once.

A failed check increments a counter and the run continues. A panic stops the run at that line. This
difference matters when reading a partial log.

Drive the user's path, not test-only setters. The harness sends real key, mouse, and focus events
through `driver_shim.cpp`, then reads the store through `lognotes-core`. The `driver` feature and the
`--livetest` flag are compiled out of normal builds, so nothing here can reach a user's app.

## Evidence

The log has two line shapes.

```text
PASS the page created by the link can be opened from search: rows=6
shot 16-page-view: ok (/tmp/lognotes-livetest/shots/16-page-view.png)
```

`PASS` and `FAIL` lines carry the label and the observed value. Shot lines carry the file name and
path. Screenshots are hand-numbered in scenario order, for example `29a-page-renamed.png`.

A proof cites the labels, the screenshots, and the stored value in the detail. `all steps passed` on
its own is not a proof of a specific feature. Open the screenshot and read the PASS line. For a
storage change, quote the database value in the detail, such as `depths=[0, 1]` or
`scheduled=[Some("2026-11-28"), None]`.

Artifacts from an earlier run are stale after a code change. Re-run before claiming the change
works. A run that panicked can still leave usable screenshots for the steps it reached.

## Cleanup

The harness exits on its own, and `verify.sh` leaves no process behind. The temp graph and shots are
wiped at the start of the next run, which is why `verify.sh` copies them into the output directory.
Keep that directory until the proof is accepted, then delete old ones.

If a run hangs, stop the process you started. Do not kill by process name, and do not delete the
output directory while it is the only evidence.

## Helpers

- `scripts/doctor.sh` runs the read-only precondition check shown under Doctor.
- `scripts/verify.sh [output-dir]` runs the full scenario and archives the log and screenshots.
