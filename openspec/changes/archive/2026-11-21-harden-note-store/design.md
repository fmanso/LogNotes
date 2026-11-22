# Design

## Context

See `proposal.md` — Why. The constraints that shape this:

- `lognotes-core` owns SQLite and has no Qt types; `lognotes-qt` owns the worker thread and the one live `Store`.
- The worker already holds `Option<Store>`, so a restore can drop and reopen it without restarting the process.
- The GUI thread holds snapshots only; it must never hold a database handle.
- The only OS-specific surface today is `database_path()` in `lognotes-qt/src/main.rs`.
- No schema change is needed; this is bookkeeping, file handling, and one new dependency for locking.
- Personal scale: an import or a composite undo holding every block of a graph is acceptable.

## Goals / Non-Goals

**Goals:**

- A failed migration leaves a usable database at its previous version, with a pre-upgrade copy.
- A backup or restore failure never leaves the user without a recoverable copy.
- One writer per data folder, and a log that explains a startup failure on a GUI-only platform.
- Import is one transaction and one undo step, with duplicate-free, explicit modes.
- Existing v0.1.0 Windows users keep their graph and backup history.

**Non-Goals:**

- Changing the schema, the block model, or the note format.
- Cloud or off-machine backups, encryption, sync.
- Page deletion or graph editing (belongs to `maintain-graph`).
- A user-facing About page (belongs to `ship-ready`); this change only puts the version in the log.
- Importing Logseq 2's database or attachments.

## Decisions

### Migrations: one transaction per version step

`migrate` reads `user_version`, refuses a newer file, then runs each pending step as:

```text
unchecked_transaction()
  apply that version's SQL
  PRAGMA user_version = <that version>
commit
```

`Connection::unchecked_transaction()` is used because `migrate` has `&Connection`; a `Transaction` guard rolls back automatically on any error. A fresh file runs the v1 step, then v2, then v3; an existing v2 file runs only v3. The final version is no longer written by a separate statement after every step.

**Why not one transaction for all steps:** a failure in v3 would roll back a fresh file's v1/v2 work and leave version 0, so a retry would hit "table already exists". Per-step transactions keep each successful step and make a retry start from the last good version. **Why not `execute_batch("BEGIN; …; COMMIT")`:** an error leaves the transaction open unless every failure path issues `ROLLBACK`; the guard is harder to get wrong.

### Pre-migration copy: `VACUUM INTO`, before the first step

After the connection is open and WAL is on, but before any migration, `Store` reads `user_version`. When `1 <= version < SCHEMA_VERSION` it writes:

```text
<backups>/pre-migration-v<from>-to-v<to>-<timestamp>.db
```

using `backup_into`, which is a consistent `VACUUM INTO` from the open connection. If the copy fails, `Store::open_with_migration_backup` returns an error and runs no migration.

**Why not copy the file with `fs::copy`:** a live database may have a `-wal` holding committed pages; copying only the main file can lose them. `VACUUM INTO` reads through the connection and produces one consistent file. **Why copy only existing graphs:** a fresh file has nothing to lose, and copying it would add a useless empty backup on every first run. **Why pass the backup directory in:** core should not invent a folder policy. `Store::open` keeps today's no-copy behavior for tests; the app calls `Store::open_with_migration_backup(path, backup_dir)`.

### Backup writes: temp file, then swap

`backup_into` changes from "delete target, then `VACUUM INTO` target" to:

```text
1. VACUUM INTO <target>.tmp
2. if <target> exists: rename <target> -> <target>.old
3. rename <target>.tmp -> <target>
4. remove <target>.old
```

If step 2 or 3 fails, the code attempts to put `<target>.old` back. The temporary and old names are derived from the target, so different backup kinds do not collide. With single-instance in place, two writers cannot race the same target.

**Alternative considered:** delete the target and write it directly (today's code, smaller). Rejected because a failed copy destroys the copy it was replacing exactly when the user needs it.

### Backup kinds and retention

Names carry the kind, so retention and the UI can tell them apart:

| Kind | Name | Keep |
| --- | --- | --- |
| Daily | `lognotes-YYYY-MM-DD.db` | 7 |
| Manual | `lognotes-YYYY-MM-DDTHHMMSS.db` | 7 |
| Pre-migration | `pre-migration-v<from>-to-v<to>-<timestamp>.db` | 3 |
| Pre-restore | `pre-restore-<timestamp>.db` | 3 |

`backup::copies` returns the path plus its kind; `prune` counts each kind separately. `backups_json` gains `"kind"`, and QML labels rows accordingly. Older `lognotes-*.db` files from v0.1.0 are classified as daily or manual by whether the name has a `T`.

**Why separate limits:** one burst of "Back up now" clicks must not evict the daily history or the pre-migration safety net. **Alternative:** a single newest-N pool; rejected because it makes retention depend on which button was pressed.

### Restore: swap files under the worker, not a restart

The worker already owns `Option<Store>`; a restore is a stop-swap-start:

```text
with the current store open:
  current --VACUUM INTO--> backups/pre-restore-<ts>.db
  chosen  --fs::copy----> <db>.restoring
  open and close a temporary Store on <db>.restoring   (validates and migrates it)
    failure: delete <db>.restoring; keep the current store open; report
  drop the store
  remove <db>, <db>-wal, <db>-shm
  rename <db>.restoring -> <db>
  Store::open_with_migration_backup(<db>, backups)
    success: snapshot, note "Restored …; previous database saved as …"
    failure: copy/rename the pre-restore copy back, reopen, report the error
```

With no open store (the database could not be opened), the pre-restore vacuum is impossible; the unopenable `<db>` and sidecars are renamed to `pre-restore-<ts>.unopenable.db` (best effort), then the chosen backup is copied in and opened. A failed open leaves the error pane with the new message, and restore remains available for another attempt.

Restoring a backup made by an older version is allowed: it is copied to a temporary path, opened there so the normal migration path (including its pre-migration copy) applies, and only then swapped into place. The backup file itself is never opened in place, so it is not migrated or modified.

**Why hot swap:** the model already treats every command as a full snapshot, so a fresh connection and snapshot is the same shape as any other update. A restart marker would add a second startup path and a "close and reopen" step for the user. **Trade-off:** the undo history of the old graph is lost; the pre-restore copy is the recovery path, and that is stated in the confirmation.

**Restore from the error pane:** the proposal names Settings, where the store is open. The case that most needs restore is an unopenable database, where Settings is unreachable. The same invokable is therefore offered on the error pane, guarded by a new `can_restore` property. It is true whenever this instance holds the data-folder lock, and false when the lock could not be acquired, because a second instance must never swap files under the first. A restore that fails validation while a store is open reports through the settings note and leaves the column open; with no store, the failure goes to the error pane.

### Single instance: one OS file lock per data folder

A new `fs2` dependency provides `FileExt::try_lock_exclusive`. `journal::start` opens or creates `<database>.lock` and tries to lock it before dispatching `Start`; the open `File` lives in `JournalRust` for the process lifetime, and the OS releases it when the process exits or crashes. If the lock is held, the model sets `error` to "LogNotes is already running", `ready` stays false, `can_restore` stays false, and no worker job opens the store. `Retry` re-attempts the lock, so closing the other window is enough.

**Why not `QLockFile`:** it is QtCore, and cxx-qt does not expose it; reaching it would add a third hand-written C++ shim. **Why not a PID file with `create_new`:** stale files after a crash would lock the user out or need stale-PID heuristics. **Why fail when the filesystem cannot lock:** a network share that cannot lock is also a poor place for a live SQLite file; a clear error is better than a silent second writer.

### Windows data path: local, with a one-time move

`database_path` on Windows prefers `%LOCALAPPDATA%`, falls back to `%APPDATA%` only if `LOCALAPPDATA` is absent, and leaves the Linux XDG logic unchanged. Before the lock and the store open, a new `prepare_data_folder(new_db)`:

- If the local database exists, do nothing (and log a warning if the roaming one also exists).
- If the local database does not exist and `%APPDATA%\LogNotes\lognotes.db` does, move `lognotes.db`, `lognotes.db-wal`, `lognotes.db-shm`, and the files in `backups/` into the local folder. `fs::rename` first; on a cross-device failure, copy then remove. If the database copy succeeds but removal of the old file fails, the local copy is used and the old one is logged, not treated as an error.
- If the move fails before a usable local database exists, return an error. `main` records it as a startup error; the model shows the error pane with both paths, and no empty graph is started.

The move helper is factored to take `(old_dir, new_dir)` so it can be tested on Linux with temp directories; only the path choice is behind `cfg!(windows)`.

### Diagnostics: a small file logger, no new dependency

A new `diagnostics` module in `lognotes-qt`:

- `init(log_path)` rotates at 1 MiB (`.log` → `.1.log`, `.1` → `.2.log`, keep three), opens append, and writes a header with `env!("CARGO_PKG_VERSION")`, `std::env::consts::OS`, and the data path.
- A process-wide `OnceLock<Mutex<Option<File>>>`; `info`, `warn`, and `error` write a timestamped line and ignore write failures. Logging must never prevent startup.
- The panic hook writes the message, location, and `Backtrace::force_capture()` before calling the previous hook.
- Log points: startup, lock result, Windows move, store error, migration copy, daily backup result, restore steps, import/export result.

**Why not `tracing`/`simplelog`:** the app has no async and one destination; rotation is three renames. A subscriber stack and its dependencies buy nothing here. **Alternative:** log only on failure; rejected because "it worked yesterday and now the window never appears" needs a normal-start line and a version.

### Import: parse everything, then one transaction, one undo entry

`export::read_folder(directory)` reads and parses every `pages/*.md` and `journals/*.md` into `ParsedPage`s before anything is written; an unreadable or non-UTF-8 file fails the whole import with its name. `Store::import_pages(pages, mode)` then:

```text
begin transaction
  ReplaceAll: collect and delete every top-level block (cascade removes children)
  for each parsed page/day:
    ensure the page (Merge replaces that page's blocks; pages not named are untouched)
    insert bullets as siblings/children, set markers and scheduled dates
  commit
push one Action::Composite(all inverses, reversed)
```

Insertion uses `apply_action` against the transaction (not the public `exec`), so `reindex` updates `block_link` and `block_fts` exactly as a normal write does, while the inverse actions are collected instead of pushed per step. The helpers that compute the next id and sibling key are refactored to free functions over `&Connection` so they can run inside the transaction. Page creation stays outside the undo history, consistent with "a page is an address".

Modes:

- **Merge** (default): for each page or day the folder names, the existing blocks are replaced by the folder's blocks; pages and days not in the folder are untouched. Re-importing the same folder is idempotent.
- **ReplaceAll**: every block in the graph is deleted first, then the folder is written; page rows not named by the folder remain but are empty. This is a destructive choice with its own confirmation.

**Why page-replace rather than append:** append is what creates silent duplicates on a second import. Content-hash dedupe is fragile across formatting and marker changes. **Why not wipe on merge:** importing one page should not delete unrelated notes. **Trade-off:** merge cannot merge two versions of the same page block-by-block; the folder's version wins. That is a clear rule and matches "the folder is the copy".

`ImportMode` is public in `export`, the bridge's `SettingsAction::Import` carries it, and QML asks after the folder dialog. The existing `import_markdown(store, dir)` signature gains the mode; core tests and the bridge are updated with it. Export is unchanged.

### UI surfaces

- Settings backups: each row gains its kind and a **Restore** button; restoring asks "Replace the notes now open? The current database is saved first." The note line reports the saved copy.
- Import: after choosing a folder, a small dialog offers **Merge** or **Replace all** (the latter warns that other pages become empty), or **Cancel**.
- Error pane: **Restore a backup…** opens a `FileDialog` starting in the backups folder, visible only when `can_restore`.
- The `Journal` model gains `can_restore`; `backups_json` gains `kind`; `restore_backup(path)` is a new invokable.

## Risks / Trade-offs

- **A full disk can now block opening an older database** → the pre-migration copy fails, the app refuses to upgrade, and the error names the folder. This is deliberate: opening without a copy risks the graph. The old file is untouched, and freeing space plus Retry works.
- **Restore deletes the live files before the new one is verified** → the pre-restore copy is written first with `VACUUM INTO`; if reopening fails, the flow copies it back. If even that fails, the error pane still allows another restore.
- **`fs2` locks can be unsupported on network filesystems** → the app reports the lock failure instead of running a second writer. Documented as "keep the database on local disk".
- **Windows move across volumes** → copy-then-remove fallback; if removal fails, both copies exist and the local one is used. The old path is logged, not silently deleted.
- **A very large import holds every inverse in memory for one undo** → acceptable at personal scale; the composite is the price of one `Ctrl+Z`. A future cap can drop the undo entry for imports, not the atomicity.
- **Logs contain file paths** → no note content is logged; paths are already shown in Settings. Rotation bounds growth.
- **A restore replaces undo history** → stated in the confirmation; the pre-restore copy is the way back.

## Migration Plan

- No schema change; a database opened by this build remains readable by v0.1.0 (same `user_version`), so rollback is a binary swap.
- Existing Windows users are moved once from `%APPDATA%` to `%LOCALAPPDATA%`; the old files are kept if removal fails, and the local one wins.
- Existing backups keep their names and are listed; new kinds add their own retention pools.
- After the behavior lands, verify and update the migration/backup claims in `architecture.md`, `lognotes-core/SPEC.md`, and `lognotes-qt/SPEC.md`; the full docs pass belongs to `ship-ready`.

## Open Questions

None that change the specs, the approach, or the task breakdown. The remaining choices are cosmetic: the exact rotation size (1 MiB) and the wording of the restore confirmation.
