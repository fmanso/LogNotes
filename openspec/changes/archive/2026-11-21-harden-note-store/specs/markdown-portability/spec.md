# Spec Delta

## Purpose

Moves a whole graph between the database and the Markdown folder Logseq 0.x reads, without losing notes or leaving a half-finished import.

## ADDED Requirements

### Requirement: Export writes the whole graph in Logseq's shape
The app SHALL write every page and every written journal day under `pages/` and `journals/`, one bullet per block, with the nesting, task markers, and `SCHEDULED` dates Logseq 0.x reads.

#### Scenario: Export a graph
- **WHEN** the user exports to a folder
- **THEN** each page is `pages/<name>.md`, each day is `journals/YYYY_MM_DD.md`, and every block, marker, schedule, and nesting level is present

#### Scenario: Names that are paths
- **WHEN** a page name contains `/`, `:`, or `\`
- **THEN** the file name escapes those characters and the page name survives a round trip

### Requirement: Export asks before replacing a folder's notes
When the destination already holds `pages/` or `journals/` notes, export SHALL ask for confirmation before writing.

#### Scenario: Non-empty destination
- **WHEN** the chosen folder already holds notes
- **THEN** the app asks before exporting

#### Scenario: Export declined
- **WHEN** the user declines the confirmation
- **THEN** no files are written

### Requirement: Import reads a Logseq 0.x folder tolerantly
The app SHALL read `pages/` and `journals/`, accepting tabs or spaces at any indent step, unknown `property::` lines, single-colon deadline lines, multi-line blocks, and Logseq's extra task markers mapped onto TODO, DOING, or DONE.

#### Scenario: A real Logseq page
- **WHEN** a page file uses tabs, an `id::` property, and a `LATER` marker
- **THEN** the blocks import with their nesting, the property is ignored, and the marker is TODO

#### Scenario: A date file name
- **WHEN** a journal file is named `2026_11_02.md` or `2026-11-02.md`
- **THEN** its blocks import into the journal day `2026-11-02`

### Requirement: Import is one atomic action
Import SHALL read every file before writing, apply the whole import in one transaction, and record it as one undo step, so a failure leaves the graph unchanged and one undo reverses a successful import.

#### Scenario: Successful import
- **WHEN** a folder imports successfully
- **THEN** one undo restores the graph to its pre-import state

#### Scenario: A file cannot be read
- **WHEN** any page or journal file is unreadable or not UTF-8
- **THEN** the import stops before writing, reports the file, and the graph is unchanged

#### Scenario: A write fails
- **WHEN** a write fails during the import
- **THEN** the transaction rolls back and the graph is unchanged

### Requirement: Import mode is explicit
The user SHALL choose between merging into the pages and days the folder names and replacing all notes. Neither mode SHALL silently append duplicates.

#### Scenario: Merge
- **WHEN** the user imports with merge and a named page already exists
- **THEN** that page's blocks are replaced by the folder's blocks, pages the folder does not name are untouched, and importing the same folder again leaves the same graph

#### Scenario: Replace all
- **WHEN** the user imports with replace all
- **THEN** every block is cleared before the folder is written, and pages the folder does not name remain but hold no blocks

#### Scenario: Cancel
- **WHEN** the user cancels the mode choice
- **THEN** nothing is read or written

### Requirement: Import and export report what moved
The app SHALL report how many pages, days, and blocks an import or export moved. An error SHALL name the file or folder that caused it.

#### Scenario: Success note
- **WHEN** an import or export succeeds
- **THEN** the settings page shows the counts

#### Scenario: Failure note
- **WHEN** an import or export fails
- **THEN** the note names the failing file or folder and the app's notes stay open
