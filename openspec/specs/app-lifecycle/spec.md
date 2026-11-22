# app-lifecycle Specification

## Purpose

Makes startup deterministic and diagnosable: one writer per data folder, a log that survives a crash, and the build version in it.

## Requirements

### Requirement: One running instance per data folder
The app SHALL acquire an exclusive lock for its data folder before opening the store. A second launch SHALL NOT open the database and SHALL tell the user that LogNotes is already running.

#### Scenario: Second launch
- **WHEN** a second instance starts while the first holds the lock
- **THEN** it shows an "already running" message, opens no store, and writes nothing to the notes

#### Scenario: Lock released
- **WHEN** the first instance exits and the user retries
- **THEN** the next instance acquires the lock and opens normally

#### Scenario: Lock unavailable
- **WHEN** the filesystem cannot provide the lock
- **THEN** the app reports the problem instead of opening a second writer

### Requirement: Startup and failures are recorded in a rotating log
The app SHALL write a log beside the database whose header names the build version, operating system, and data path. It SHALL record startup, store failures, migrations, backups, restores, imports, exports, and panics. A log that cannot be written SHALL NOT prevent the app from starting.

#### Scenario: Normal start
- **WHEN** the app opens successfully
- **THEN** the log header names the version and data path, and a startup line is present

#### Scenario: Store failure
- **WHEN** the database cannot be opened
- **THEN** the log records the error and path, and the user sees the same error

#### Scenario: Panic
- **WHEN** the app panics
- **THEN** the panic message and location are written to the log before the process exits

#### Scenario: Unwritable log
- **WHEN** the log file cannot be created
- **THEN** the app still opens the notes

### Requirement: The log rotates and keeps a short history
The log SHALL rotate when it exceeds a fixed size and SHALL keep the newest three files, so a long-running install cannot fill the disk.

#### Scenario: Rotation
- **WHEN** the log passes its size limit at startup
- **THEN** the old log is renamed, a new one starts, and at most three log files remain

#### Scenario: No old logs
- **WHEN** no log exists
- **THEN** rotation creates the first log without error
