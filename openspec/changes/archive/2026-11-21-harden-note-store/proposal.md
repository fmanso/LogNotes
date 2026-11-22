# Proposal

## Why

LogNotes keeps every note in one SQLite file, and a few failure paths can make that file unopenable or silently lose work. The safety net is incomplete:

- **Migrations are not atomic.** Each schema step runs as autocommitted statements and `user_version` is bumped only at the end (`lognotes-core/src/store.rs:1003-1021`). v3 uses non-idempotent `ALTER TABLE ADD COLUMN` (`store.rs:82-83`), so a crash or full disk mid-migration leaves a file that cannot be reopened.
- **There is no pre-migration copy.** The daily backup is written after the store opens (`lognotes-qt/src/journal.rs:1100-1109`), so the newest copy can predate the migration by a day; the state that failed is not the state saved.
- **Backups cannot be restored in the app.** Settings lists copies and offers "Back up now" (`lognotes-qt/qml/main.qml:2088-2150`), but restoring means quitting and copying a file by hand.
- **A failed backup can delete the previous one.** `backup_into` removes the destination before copying (`store.rs:667-674`).
- **Two instances can share the file.** WAL prevents corruption, but stale snapshots overwrite each other's edits, and both instances race backup and prune.
- **Import is not one action.** Every write inside an import — insert, indent, marker, scheduled date — is its own transaction and undo entry (`export.rs:62-88,155-188`), so failure leaves a partial graph, `Ctrl+Z` undoes one step rather than the import, and importing the same folder twice silently duplicates every block.
- **Windows stores the live database in roaming `%APPDATA%`** (`lognotes-qt/src/main.rs:53-77`) — the wrong place for SQLite under profile sync.
- **Startup failures are invisible.** There is no log, panic hook, or version line to diagnose a failure on a GUI-only platform.

Why now: v0.1.0 is published. Every later release makes the migration path more expensive to get wrong, and a damaged database cannot be repaired after the fact.

## What Changes

- Make each migration step and its `user_version` bump one transaction; copy an older database before upgrading it; add fixtures that open a v1 and a v2 file and upgrade them. Keep refusing a schema newer than the binary.
- Write backups to a temporary file and rename into place, so a failed copy never destroys the copy it replaces.
- Add "Restore this backup" to Settings: confirm, set the current file aside, restore, restart.
- Enforce one running instance per data folder; a second launch explains instead of opening a second writer.
- Make import one transaction and one undo step; make merge-versus-replace an explicit choice; report what was imported; leave the graph untouched on failure.
- Move the Windows database and its backups folder to `%LOCALAPPDATA%`, moving an existing `%APPDATA%` pair once so current users keep their graph and backup history.
- Write a rotating log beside the database, install a panic hook, and put the build version in the log header.
- Decide and document retention for manual copies versus daily copies.

## Capabilities

### New Capabilities

- `note-storage`: where the database lives, safe schema migration, backup integrity, and restore.
- `app-lifecycle`: single-instance startup, crash and error diagnostics, and version identification in logs.
- `markdown-portability`: import and export behavior, including atomic import and explicit merge/replace semantics.

### Modified Capabilities

None. No specs exist yet.

## Impact

- `lognotes-core`: `store.rs` migration and backup, `export.rs` import.
- `lognotes-qt`: `main.rs` data path, `journal.rs` startup and import jobs, `backup.rs`, Settings QML.
- Tests: migration fixtures (new), import atomicity and duplication, restore, single instance.
- One-time Windows data-path move; no network, server, or schema redesign.
- Verify and update the migration and backup claims in `architecture.md` and both `SPEC.md` files once the behavior lands; the full docs pass belongs to `ship-ready`.
