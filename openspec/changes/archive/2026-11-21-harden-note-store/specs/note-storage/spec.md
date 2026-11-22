# Spec Delta

## Purpose

Keeps the notes in one SQLite file that can be found, upgraded, copied, and restored without losing the graph.

## ADDED Requirements

### Requirement: The database lives in local application data
On Windows the live database and its backups SHALL live under `%LOCALAPPDATA%\LogNotes`. On Linux they SHALL live under `$XDG_DATA_HOME/lognotes`, or `~/.local/share/lognotes` when `XDG_DATA_HOME` is unset.

#### Scenario: Linux data location
- **WHEN** the app starts on Linux with `XDG_DATA_HOME` set
- **THEN** the database is `$XDG_DATA_HOME/lognotes/lognotes.db` and backups are in its `backups` folder

#### Scenario: Windows data location
- **WHEN** the app starts on Windows
- **THEN** the database is under `%LOCALAPPDATA%\LogNotes`, not roaming `%APPDATA%`

### Requirement: An existing Windows graph moves once to the local path
When the local Windows database does not exist but a graph exists under `%APPDATA%\LogNotes`, the app SHALL move the database, its SQLite sidecars, and the backups folder to the local path before opening the store.

#### Scenario: Existing roaming graph
- **WHEN** `%APPDATA%\LogNotes\lognotes.db` exists and `%LOCALAPPDATA%\LogNotes\lognotes.db` does not
- **THEN** the graph and its backups are moved, and the app opens the same notes from the local path

#### Scenario: Move fails
- **WHEN** the move cannot complete
- **THEN** the app reports an error naming both paths and does not start an empty graph

#### Scenario: Both locations hold a database
- **WHEN** both the roaming and local paths hold a database
- **THEN** the app uses the local one and leaves the roaming copy untouched

### Requirement: Schema upgrades are atomic
Each schema version step and its `user_version` update SHALL commit in one transaction, so a failed step leaves the database at the previous version and still usable.

#### Scenario: An older file upgrades
- **WHEN** a version 1 or version 2 database is opened
- **THEN** it reaches the current version with its pages, blocks, links, and settings intact

#### Scenario: A step fails
- **WHEN** a statement in the current step fails
- **THEN** that step's changes roll back, `user_version` stays at the previous version, and the app reports the error

#### Scenario: A newer schema
- **WHEN** the file's `user_version` is newer than the binary supports
- **THEN** the app refuses to open it and leaves the file untouched

### Requirement: An existing database is copied before upgrade
Before upgrading a database that already has a schema version, the app SHALL write a consistent pre-migration copy into the backups folder. If that copy cannot be written, the upgrade SHALL NOT run.

#### Scenario: Upgrade makes a copy
- **WHEN** a version 2 database is upgraded
- **THEN** a pre-migration copy that opens at version 2 exists in the backups folder, and the live file is current

#### Scenario: Copy cannot be written
- **WHEN** the backups folder cannot accept the copy
- **THEN** the app refuses to upgrade, reports why, and leaves the live database at its old version

#### Scenario: Fresh database
- **WHEN** the app creates a database for the first time
- **THEN** no pre-migration copy is required

### Requirement: A backup write never destroys the previous copy
Writing a backup SHALL prepare the new copy under a temporary name and only replace the target after the copy is complete. A failure SHALL leave the previous file in place.

#### Scenario: Successful backup
- **WHEN** a backup is written
- **THEN** the target is a complete SQLite database and no temporary file remains

#### Scenario: Failed backup
- **WHEN** the copy fails partway
- **THEN** the previous backup at that path is unchanged and the app reports the failure

### Requirement: A backup can be restored from the app
The app SHALL restore a chosen backup by saving the current database aside, replacing the live file, and reopening it. A failed restore SHALL leave the previous notes recoverable.

#### Scenario: Restore from settings
- **WHEN** the user restores a backup from Settings
- **THEN** the notes show the backup's contents and the pre-restore database is kept in the backups folder

#### Scenario: Restore after a failed open
- **WHEN** the database cannot be opened and the user restores a backup from the error screen
- **THEN** the backup is opened and the unopenable file is kept aside

#### Scenario: Restore fails
- **WHEN** the chosen file is not a usable database or the copy fails
- **THEN** the app reports the error and the previous notes remain available or recoverable

### Requirement: Backup retention is per kind
Daily, manual, pre-migration, and pre-restore copies SHALL each have their own retention limit, so one kind cannot evict another.

#### Scenario: Daily retention
- **WHEN** more than seven daily copies exist
- **THEN** the seven newest daily copies are kept and older ones are removed

#### Scenario: Manual retention is independent
- **WHEN** many manual copies are written
- **THEN** daily and safety copies are not removed to make room

#### Scenario: Safety retention
- **WHEN** more than three pre-migration or pre-restore copies exist
- **THEN** the newest three of each kind are kept
