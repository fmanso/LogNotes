# Design

## Context

See `proposal.md` — Why. This is the last of three phases: `harden-note-store` makes the data trustworthy, `maintain-graph` makes the graph maintainable, and this change makes the build supportable and the record consistent.

Current constraints:

- CI (`ci.yml`) runs the workspace tests and packages on Linux, but the Windows job only builds and packages.
- `make livetest` runs the GUI scenario on the developer's display through a `systemd-run` slice; CI has no user systemd session.
- The app has no version surface; `Cargo.toml` is the only version.
- The release workflow publishes on any `v*` tag, without checking that the tag matches `Cargo.toml`.
- Four markdown documents carry the design record; OpenSpec's `specs/` will start filling when the two earlier changes archive.
- The tool is built for one person, on Linux and Windows, with no server.

## Goals / Non-Goals

**Goals:**

- A user can see which build they are running, without leaving the app.
- A release cannot be published under a version the binary does not claim.
- Windows gets the same unit-test check Linux gets.
- The GUI scenario runs on a schedule, with screenshots, without blocking every PR.
- The distribution path is decided and documented, not implied.
- The markdown record and the OpenSpec specs stop disagreeing.

**Non-Goals:**

- Buying or configuring a code-signing certificate.
- Building a Windows installer, AppImage, or Flatpak in this change.
- macOS, auto-update, telemetry, or a crash-reporting service.
- Porting architecture rationale into behavior specs.

## Decisions

### Version surface: one property, one Settings section

`Journal` gains a `version` qproperty filled from `env!("CARGO_PKG_VERSION")` in `start`/`fill_paths`; QML shows it in a new **ABOUT** section at the bottom of Settings, alongside the platform from `Qt.platform.os`, a one-line description, and the MIT licence. The version is read from the build, not typed into `main.qml`.

**Why Settings:** it is already the support page — data path, backups, import/export. A separate About window adds a surface and a navigation path for one line. **Why not the window title:** it is easy to miss and the title is not where a bug report looks. **Alternative:** a version command-line flag; rejected because the app is launched from the desktop, not a shell.

### Release check: a job before the build

`release.yml` gains a `version` job that checks out the tag (or the `workflow_dispatch` input), reads the workspace version from `Cargo.toml`, and compares it to `v<version>`. `build` depends on it, so a mismatch fails before any artifact is built.

**Why before the build:** a release that publishes `v0.2.0` with a `0.1.0` binary is worse than a red workflow, and failing after two platform builds wastes the expensive part. **Limitation:** in the `workflow_dispatch` path the reusable build still checks out the default branch; the check guarantees the tag names the same version, which is the property that matters for the artifacts' names.

### Windows tests: the workspace, in release

The Windows job gains `cargo test --workspace --release --jobs 2` after Qt is installed and before packaging. Release mode reuses the artifacts the packaging step needs. The Qt `bin` directory is already on `PATH`.

**Why the workspace:** the bridge tests are pure model tests — they open in-memory stores and never create a window — so they should run on the platform with the least coverage. **If a test does need a GUI on Windows:** move that test to the Linux job and keep the core plus model tests on Windows; do not silently skip the job. **Alternative:** run only `lognotes-core`; rejected because the bridge is where the platform-specific C++/Qt glue lives.

### GUI scenario: scheduled, non-blocking at first

A new `.github/workflows/livetest.yml` runs on a schedule and on `workflow_dispatch`:

```text
Xvfb : server 1600x1200x24
QT_QUICK_BACKEND=software
QSG_RENDER_LOOP=basic
TMPDIR=${{ runner.temp }}
cargo run -p lognotes-qt --features driver -- --livetest
```

It does **not** call `make livetest`, because the Makefile's `systemd-run` slice does not exist on a GitHub runner; the Makefile stays the developer entry point. Screenshots from `$TMPDIR/lognotes-livetest` are uploaded on every run. The job is `continue-on-error: true` until it has a stable streak; removing that is a one-line promotion.

**Why scheduled:** the scenario drives a real window and a compositor-sensitive input path; making it a required PR check on day one would trade a useful signal for a flaky gate. **Why software rendering:** Xvfb has no GPU; the scene graph's basic loop and software backend are the combination the harness was written for. **Alternative:** run it inside the Linux job; rejected because a flaky GUI step would block every package build.

### Distribution: documented portable builds, installer deferred

The supported artifacts stay the Linux tarball and the Windows zip. README gains an install section for the tarball (binary, `.desktop`, icon, README, licence), and a Windows note that the zip is portable and that SmartScreen may warn because the binary is unsigned. The decision records that:

- no AppImage or Flatpak is built in this change (the tarball is the documented Linux path);
- no Windows installer is built until there is a code-signing certificate, because an unsigned installer adds no trust over the zip;
- signing is the prerequisite for a future one-click installer.

**Why:** the app is distributed to one person; an installer without a signature is a different shape of the same warning, and AppImage/Flatpak add packaging dependencies without changing daily use. **Alternative:** an unsigned Inno Setup installer; rejected for the trust reason and because it would add a Windows packaging tool to CI for no practical gain. This is a decision worth revisiting if distribution widens.

### Docs: OpenSpec is the behavior record; markdown is the design record

The archive step copies each change's delta specs into `openspec/specs/`. From this change on:

- `openspec/specs/<capability>/spec.md` is the **behavior contract** — the place to check what the app must do.
- `architecture.md` and the two `SPEC.md` files are **architecture, rationale, and module contracts** — why the pieces are shaped this way, and what each crate owns. They may describe implementation, but they may not contradict a behavior spec.
- `goal-and-requirements.md` stays the product goal: the problem, the capabilities, the non-goals, and the unconfirmed inferences. It is not a second behavior spec.
- README points at both, and `openspec/config.yaml` gains a short `context` note so future agents apply the same split.

The truth pass searches for the known stale claims instead of trusting line numbers (the earlier phases move them): import/export scope, migration transactionality, the `move`/`set_collapsed` commands, collapsed-row omission, the removed `markdown_shim.cpp`, the removed Pages popup, the data path, and the known limitations that `maintain-graph` retires. Each is either corrected or pointed at the spec that now owns it.

**Why not port the corpus:** the four documents carry design reasoning that a behavior delta should not absorb, and porting them would create a third copy during the transition. **Why not leave the docs alone:** once `openspec/specs` exists, two behavior records that disagree are worse than one incomplete record.

### No new dependencies

The version property, About section, CI jobs, and release check need no crate or service. The livetest workflow uses `xvfb` from the runner image.

## Risks / Trade-offs

- **The GUI scenario is flaky under Xvfb** → it runs scheduled, `continue-on-error`, and uploads screenshots; promote only after a stable streak. The developer's `make livetest` remains the authoritative local run.
- **Windows tests fail for Qt runtime reasons, not code** → `PATH` already carries Qt `bin`; if a bridge test needs a display, move that one test to Linux and say so, rather than dropping the Windows job.
- **The docs pass expands into a rewrite** → scope it to the claims this and the earlier changes invalidate; keep rationale intact. Cross-checking against `openspec/specs` is the exit condition.
- **The distribution decision disappoints the "production ready" expectation** → it is explicit and reversible: a signed installer is a follow-up with a clear prerequisite, and the README explains the current install path rather than leaving it implied.
- **Version duplication returns** → one source, `CARGO_PKG_VERSION`; QML never hardcodes a number, and the release check enforces the tag.
- **`workflow_dispatch` releases build the default branch** → the version check still requires the tag and package version to agree; document that the tag push is the normal path.

## Migration Plan

- No schema, note-format, or database change.
- CI changes are additive: Windows tests, a new scheduled workflow, and a pre-build version job.
- Docs edits are limited to the stale claims and the new "where the specs live" note.
- The app changes only by adding the version property and the About section; no user data or migration.
- Rollback is a workflow/docs revert; the app version surface is inert.

## Open Questions

None that change the specs, the approach, or the task breakdown. Obtaining a code-signing certificate is an external, later decision; the installer work is explicitly deferred until it exists. The livetest promotion is a follow-up after a stable streak, not a design unknown.
