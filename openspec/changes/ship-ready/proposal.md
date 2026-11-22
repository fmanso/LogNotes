# Proposal

## Why

The build works on both platforms, but only Linux is tested, and the last mile of support is missing:

- **Windows is built, never exercised.** CI compiles and packages it (`ci.yml:78`) but runs no tests there, even though the core suite needs no Qt.
- **The richest regression suite runs by hand.** `make livetest` drives the real window through typing, links, search, themes, and slash commands (`lognotes-qt/SPEC.md:345-425`), but it is not part of CI, so input-handling regressions are found by a person or not at all.
- **There is no installer, signing, or version surface.** Windows ships a zip that will meet SmartScreen; Linux ships a tarball; the app cannot say which version it is.
- **The design record has drifted.** `architecture.md:162-165` and `goal-and-requirements.md:99-104` say import/export are absent; `lognotes-core/SPEC.md:117,121,133,224` documents a `move` command, a `set_collapsed`, collapsed-row omission, and transactional migrations that the code does not have; `lognotes-qt/SPEC.md:33,273` names a removed shim and a removed popup.
- **OpenSpec has no specs.** The root was initialized with empty `specs/` and `changes/`; the real corpus still lives in four markdown documents, so every new change describes capabilities that have no spec.

Why now: v0.1.0 is public and the next phases add behavior. Without this pass the docs will describe two different apps, and the release will remain hard to support and install.

## What Changes

- Run the core tests and the bridge tests on Windows CI, not only the release build.
- Run `make livetest` on a scheduled CI job under Xvfb, starting non-blocking and promoting it once it is reliable.
- Add an About/version surface in the app and carry the release version into it.
- Decide and record the distribution path: Windows installer and code signing; Linux AppImage/Flatpak or a documented tarball install. Building every option is not required — making the decision is.
- Do a docs truth pass: correct the stale import/export scope, migration, collapse, command, shim, and popup claims in `architecture.md`, `goal-and-requirements.md`, `lognotes-core/SPEC.md`, and `lognotes-qt/SPEC.md`.
- Decide whether to port that corpus into OpenSpec capabilities or to keep the markdown as the durable record and use OpenSpec only for in-flight changes; then follow one rule.

## Capabilities

### New Capabilities

- `app-identity`: the running app identifies its build version and exposes an About surface.

### Modified Capabilities

None. No specs exist yet.

## Impact

- CI workflows and packaging scripts.
- The QML shell (About/version) and the bridge property that carries the version.
- Documentation only for the drift pass; no note-format or database changes.
- OpenSpec organization: a one-time decision about where the durable specs live.
