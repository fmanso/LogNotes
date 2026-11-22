# Tasks

## 1. Atomic migrations

- [x] 1.1 Rewrite `migrate` in `lognotes-core/src/store.rs` so each pending version step runs in `unchecked_transaction()` and sets `user_version` inside that same transaction; verify a v2 fixture reaches v3 and `cargo test -p lognotes-core` passes.
- [x] 1.2 Add `lognotes-core/tests/fixtures/v1.sql` and `v2.sql` with the old schemas, sample page/block/link rows, and the matching `PRAGMA user_version`; verify each fixture opens through a raw `rusqlite` connection at its declared version.
- [x] 1.3 Add `lognotes-core/tests/migrations.rs` covering v1→current and v2→current upgrades, preservation of pages, blocks, links, and FTS, and refusal of a newer `user_version`; verify with `cargo test -p lognotes-core --test migrations`.
- [x] 1.4 Add an atomicity test using a v2 fixture that already contains a conflicting `setting` table: v3 must fail, `user_version` must stay 2, and the `favourite` column must be absent; verify the test passes and a retry after dropping the conflicting table succeeds.
- [x] 1.5 Update the migration section of `lognotes-core/SPEC.md` to state the per-step transaction and `user_version` placement; verify the wording matches `migrate` and the new tests.

## 2. Pre-migration copy

- [x] 2.1 Add `Store::open_with_migration_backup(path, backup_dir)` and keep `Store::open` as the no-copy form used by tests; verify all existing core tests still pass unchanged.
- [x] 2.2 Before the first step, write `<backup_dir>/pre-migration-v<from>-to-v<to>-<timestamp>.db` with `VACUUM INTO` when `1 <= user_version < SCHEMA_VERSION`, and abort the open if the copy fails; verify against a v2 fixture with a real backup directory.
- [x] 2.3 Add tests proving the copy exists and opens at its old version, an unusable backup directory (a path whose parent is a file) leaves the live file at v2 with an error, and a fresh database writes no copy; verify with `cargo test -p lognotes-core --test migrations`.
- [x] 2.4 Call `Store::open_with_migration_backup` from `Job::Start` in `lognotes-qt/src/journal.rs`, passing `backup::directory(database)`; verify `cargo test -p lognotes-qt` and a manual launch against a copied v2 database.
- [x] 2.5 Update the migration/backup paragraphs in `lognotes-core/SPEC.md` and the storage section of `architecture.md` to name the pre-migration copy; verify the documents match the tests.

## 3. Backup integrity and retention

- [x] 3.1 Rewrite `Store::backup_into` to write `<target>.tmp` with `VACUUM INTO`, rename the existing target aside, rename the temp into place, and remove the old copy, restoring the old one on failure; verify a test overwrites an existing target and leaves a complete database.
- [x] 3.2 Add a failure test where `<target>.tmp` is a directory so `VACUUM INTO` fails; verify the previous target is byte-for-byte unchanged and no partial file replaces it.
- [x] 3.3 Add daily, manual, pre-migration, and pre-restore kinds to `lognotes-qt/src/backup.rs`, name them as designed, return the kind from `copies`, and prune 7 daily, 7 manual, and 3 of each safety kind; verify tests for 10 daily copies, manual independence, and safety caps.
- [x] 3.4 Add `"kind"` to `backups_json` and label each Settings row by kind and size; verify `cargo test -p lognotes-qt` and the Settings page listing each kind.
- [x] 3.5 Update the backup claims in `lognotes-qt/SPEC.md` and `lognotes-core/SPEC.md` (atomic write, per-kind retention) and verify they match `backup.rs`.

## 4. Restore

- [x] 4.1 Add `backup::restore_database(store: Option<&Store>, live, chosen, backups)` that writes the pre-restore copy, copies the chosen file to `<live>.restoring`, validates it by opening and closing a temporary `Store`, keeps the current store open on validation failure, and only then swaps the files; verify unit tests for success, an invalid chosen file leaving live files untouched, and an unopenable live file being set aside.
- [x] 4.2 Add `SettingsAction::Restore { backup }`, the `restore_backup` invokable, and the worker branch that calls 4.1, reopens the store, and delivers a fresh snapshot. A validation failure with a store open must report through the settings note and leave the column open; with no store it reports through `error`. Verify `cargo test -p lognotes-qt`.
- [x] 4.3 Add the `can_restore` property (default false) and the error-pane visibility rule; verify a QML/unit check with both values. Task 5.2 sets it true once the data-folder lock is held and false when acquisition fails.
- [x] 4.4 Add a Restore button to each Settings backup row with a confirmation that the current database is saved first, and add "Restore a backup…" to the error pane with a `FileDialog` starting in `journal.backup_folder`, visible only when `can_restore`. Verify through the livetest invokable and screenshots.
- [x] 4.5 Add a livetest step that prepares two databases, restores the second, and asserts the first is saved as a pre-restore copy; verify with `make livetest`.
- [x] 4.6 Update the restore, error-pane, and failure paragraphs in `lognotes-qt/SPEC.md`; verify they match the worker flow and `can_restore` semantics.

## 5. Single instance

- [x] 5.1 Add `fs2` to `lognotes-qt/Cargo.toml` and a `lock_data_folder(database) -> Result<File>` helper that locks `<database>.lock` exclusively and non-blocking; verify a test acquires it twice, sees the second fail, then succeeds after the first is dropped.
- [x] 5.2 Hold the lock in `JournalRust` for the process lifetime, acquire it in `start` before dispatching `Start`, set `error` to "LogNotes is already running" and `can_restore` false on failure, set `can_restore` true once the lock is held, and let `Retry` try again; verify the helper test and a manual two-launch check.
- [x] 5.3 Document the one-writer invariant in `architecture.md`; verify the document. The lock log line is added with the diagnostics wiring in 8.3.

## 6. Windows data path

- [x] 6.1 Factor the Windows path choice into a testable helper that prefers `%LOCALAPPDATA%`, falls back to `%APPDATA%`, and leaves the XDG path unchanged on Linux; verify unit tests for each input.
- [x] 6.2 Add `migrate_windows_graph(old_dir, new_dir)` that moves the database, `-wal`, `-shm`, and backup files, falls back to copy-then-remove, uses the local copy when both exist, and errors before an empty graph can start. Verify temp-directory tests for the move and both-exist cases, plus a direct test of the copy-then-remove fallback.
- [x] 6.3 Wire the path helper and move into `main`/startup so a failure becomes a startup error in the error pane; verify the helper tests and `cargo build -p lognotes-qt`, and record the Windows-only manual check for CI or a Windows machine.
- [x] 6.4 Update the data-location paragraphs in `README.md`, `architecture.md`, and `lognotes-qt/SPEC.md`; verify they name `%LOCALAPPDATA%` and the one-time move.

## 7. Import: one action, explicit mode

- [x] 7.1 Split `export.rs` into `read_folder` (parse every file before writing) and `Store::import_pages` (write); verify an unreadable or non-UTF-8 file returns an error before any row changes.
- [x] 7.2 Add `ImportMode::{Merge, ReplaceAll}` and run `Store::import_pages` in one `Transaction`, collecting inverse actions into one `Action::Composite`; refactor the id and sibling-key helpers to take `&Connection`. Verify importing several blocks takes exactly one undo to restore the pre-import graph.
- [x] 7.3 Implement Merge as page-level replacement and ReplaceAll as clear-all-then-write; verify idempotent re-import, untouched pages in Merge, empty unnamed pages in ReplaceAll, and rollback on a write failure.
- [x] 7.4 Change the public `import_markdown` signature to take the mode, update the core re-export, round-trip test, and export/import tests; verify `cargo test -p lognotes-core`.
- [x] 7.5 Carry the mode through `SettingsAction::Import` and the bridge; in QML, after the folder dialog, ask Merge or Replace all (with a destructive warning) or Cancel, and show the counts in the note. Verify `cargo test -p lognotes-qt` and the note text.
- [x] 7.6 Add livetest coverage for importing a small folder in each mode by invoking the dialog path directly; verify with `make livetest`.
- [x] 7.7 Update the export/import sections of `lognotes-core/SPEC.md`, `lognotes-qt/SPEC.md`, and the goal/architecture out-of-scope claims; verify they match the modes and the one-undo rule.

## 8. Diagnostics

- [x] 8.1 Add `lognotes-qt/src/diagnostics.rs` with `init`, `info`, `warn`, `error`, a 1 MiB rotation keeping three files, a header carrying version, OS, and data path, and a panic hook writing the message, location, and backtrace; verify rotation and header tests in a temp directory.
- [x] 8.2 Make logging best-effort: a log that cannot be created or written must not stop startup; verify a test with an unwritable path.
- [x] 8.3 Initialize diagnostics in `main` and log startup, lock result, Windows move, store open, migration copy, daily backup, restore, import, and export; verify a normal launch writes the expected lines and `make run` still starts.
- [x] 8.4 Add the log location to `README.md` and `lognotes-qt/SPEC.md`; verify the documented path matches the code.

## 9. Integration verification

- [x] 9.1 Run `make test` and fix any failure; verify the whole workspace passes.
- [x] 9.2 Run `make livetest` and verify the existing scenario plus the new restore and import steps pass with screenshots.
- [x] 9.3 Walk the three spec files scenario by scenario against the implementation; verify each has a test, a livetest step, or a documented manual check.
- [x] 9.4 Re-read `architecture.md`, both `SPEC.md` files, and `README.md` for contradictions introduced or left by this change; fix only stale claims from this change.

## Workflow follow-up

- Archive this change after review and verification, then check the archived specs.
