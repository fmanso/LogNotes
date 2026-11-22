# Tasks

## 0. Prerequisite

- [x] 0.1 Confirm `harden-note-store` and `maintain-graph` are implemented and merged, since the docs pass corrects claims those phases change; verify `openspec list --all` or `git log`, and stop to apply them first if either is still only planned.

## 1. App identity

- [x] 1.1 Add a `version` qproperty filled from `env!("CARGO_PKG_VERSION")` when the model starts, and verify a livetest reads it through `journal_property("version")` and that it equals the workspace version.
- [x] 1.2 Add an **ABOUT** section to Settings with the version, the platform from `Qt.platform.os`, a one-line description, and the MIT licence, with an `objectName` for the version line; verify the livetest reads the line and the Settings page still fits.
- [x] 1.3 Update `README.md` and `lognotes-qt/SPEC.md` with the About surface and the version source; verify the documented version comes from `Cargo.toml`, not QML.

## 2. Release version gate

- [x] 2.1 Add a `version` job to `release.yml` that checks out `${{ inputs.tag || github.ref }}`, reads the workspace version from `Cargo.toml`, compares it to the tag, and makes `build` depend on it; verify the workflow parses and a matching release reaches the build job.
- [x] 2.2 Test the comparison locally with a matching and a mismatched `TAG` value, and record the `workflow_dispatch` checkout caveat in the workflow comment; verify the mismatched case exits non-zero before any artifact name is produced.

## 3. Windows tests

- [x] 3.1 Add `cargo test --workspace --release --jobs 2` to the Windows job after Qt is installed and before packaging; verify the Windows CI job runs and reports the workspace tests.
- [x] 3.2 If a bridge test needs a display on Windows, move only that test to the Linux job and leave the core and model tests on Windows; verify the Windows job still runs the remaining tests rather than being skipped.

## 4. GUI scenario in CI

- [x] 4.1 Add `.github/workflows/livetest.yml` on a schedule and `workflow_dispatch`, with Xvfb, `QT_QUICK_BACKEND=software`, `QSG_RENDER_LOOP=basic`, `TMPDIR=${{ runner.temp }}`, the direct `cargo run -p lognotes-qt --features driver -- --livetest` command, screenshot upload on every run, and `continue-on-error: true`; verify a dispatched run starts and the screenshots upload.
- [x] 4.2 Run the workflow once to completion and inspect the screenshot artifact and log; record the promotion criterion (a stable streak, then remove `continue-on-error`) in the workflow comment.

## 5. Distribution decision

- [x] 5.1 Add a README install section: unpack the Linux tarball into `~/.local`, install the binary, `.desktop`, and icon, and note the PATH requirement; for Windows, unpack the zip and explain the SmartScreen warning for an unsigned binary. Verify the commands run on a clean directory.
- [x] 5.2 Record the decision that no AppImage, Flatpak, or Windows installer is built in this change, and that code signing is the prerequisite for a future installer; verify `README.md` and the packaging section of `lognotes-qt/SPEC.md` say the same thing.

## 6. Docs truth pass and corpus rule

- [x] 6.1 Add a "Where the specs live" note to `README.md` and a short `context` entry to `openspec/config.yaml`: OpenSpec specs are the behavior contract; the markdown docs are architecture, rationale, and module contracts, kept consistent with the specs. Verify both files parse and agree.
- [x] 6.2 Correct `architecture.md`: remove import/export from "out of scope", state the Windows `%LOCALAPPDATA%` path and the one-writer rule, describe atomic migrations and pre-migration copies, honour collapsed rows, and change "the database is the only persistent state" to name backups and logs as derivative state. Verify no claim contradicts the archived specs.
- [x] 6.3 Correct `goal-and-requirements.md`: remove the "export path left open" and "no importer" technical notes, keep the out-of-scope list for sync/plugins/queries, and make the capability text match what shipped. Verify the document no longer contradicts itself.
- [x] 6.4 Correct `lognotes-core/SPEC.md`: make `move` and `set_collapsed` real or remove them if the approach changed, fix the `pages()` journal contradiction, describe collapsed-row omission, per-step migration transactions, pre-migration copies, and the import mode. Verify against the code and tests.
- [x] 6.5 Correct `lognotes-qt/SPEC.md`: remove the `markdown_shim.cpp` and Pages-popup references, retire the folding/block-reorder/link-autocomplete limitations, keep the Backspace-merge omission, and add the data path, log, single instance, restore, import mode, About/version, and livetest-in-CI behavior. Verify against the code.
- [x] 6.6 Cross-check with searches for the stale phrases (`markdown_shim`, `Pages popup`, `No importer`, `export path is left open`, `move(block`, `set_collapsed`, `collapsed block is included`, `%APPDATA%` except the migration story) and verify each hit is either corrected or intentionally historical.

## 7. Integration verification

- [x] 7.1 Run `make test` and fix every failure; verify the whole workspace passes.
- [x] 7.2 Run `make livetest` and verify the existing scenario plus the About/version steps pass with screenshots.
- [x] 7.3 Let Linux and Windows CI run on a push and verify both are green; dispatch the livetest workflow and verify it completes or reports a documented flake with screenshots.
- [x] 7.4 Run the release version check against the current version and a deliberately wrong tag; verify the wrong tag fails before the build and the right one passes.
- [x] 7.5 Re-read README, `architecture.md`, both `SPEC.md` files, and the archived specs for contradictions; verify the only remaining historical statements are explicitly marked as such.

## Workflow follow-up

- Promote the livetest workflow to blocking after a stable streak, then remove `continue-on-error`.
- Archive this change after review and verification, then check the archived specs.
