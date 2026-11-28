# Settings and portability

The settings page names the database and backup folders, offers a backup and a restore, and exports
or imports the graph as a Logseq 0.x Markdown folder. Restore saves the previous database first.

## Sub-features

- `settings-open` opens the settings page from the gear and closes with Escape.
- `settings-paths` names the database file, the backups folder, and the build version.
- `font-choice` picks the note font and keeps a family that is not installed.
- `backup-now` writes a backup copy and lists it.
- `restore-offer` offers a restore and swaps in the chosen graph.
- `restore-safety` saves the previous database as a pre-restore copy.
- `export-markdown` writes `journals/` and one file per page.
- `import-merge` brings in a Logseq day with its marker, schedule, and child.
- `import-replace` clears the old graph and writes the folder's.

## How to get to it (user POV)

- Click the gear in the header. Press Escape to close.
- Choose Change… in APPEARANCE, filter the font list, and pick a family.
- Choose Back up now, then Restore, and confirm.
- Choose Export and confirm the folder.
- Choose Import, pick Merge or Replace, and confirm.

## Driving it with livetest

Preconditions:

- `scripts/doctor.sh` passes.
- `scripts/verify.sh` has produced `livetest.log` and `shots/`.
- The scenario uses `${TMPDIR:-/tmp}/lognotes-livetest` for the database, backups, exports, and the
  import fixture. It never touches the real graph.

- **Open settings.** Read `PASS the gear opens the settings page: focus=settingsPage`. Shot
  `30-settings`.
- **Paths and version.** Read `PASS it names the database file`,
  `PASS it names the backups folder`, `PASS the About section names the build and the version is the
  package version`, and `PASS the app can offer a restore`. The values must point inside the
  livetest temp directory.
- **Note font.** Read `PASS the font picker opens with the filter focused`,
  `PASS typing in the font filter lists matches`, and
  `PASS selecting a font previews it without applying`. Shot `30a-font-selected` shows the
  highlighted family in the preview card.
  Read `PASS applying a font sets the notes and is remembered`. Shots `30a-font-dialog-light` and
  `30c-font-applied`; the detail must repeat the chosen family in `picked=`, `note=`, and `stored=`,
  and say `rendered=true` for the rendered note text.
  Read `PASS the Default entry restores the app font`.
- **Unavailable font.** Read `PASS an unavailable font falls back to the app font` (the note font
  equals the captured default while the store keeps the missing name) and
  `PASS the settings row says the font is not installed`. Shot `30d-font-missing`. Then read
  `PASS picking Default clears an unavailable font`.
- **Backup now.** Read `PASS back up now writes a copy and lists it: before=1 after=2`.
- **Export.** Read `PASS export writes bullets into journals/ and a file per page`. Shot
  `31-exported`. Check the archived export path in the detail.
- **Import merge.** Read `PASS import brings a logseq day in, with its marker, schedule and child`.
  Shot `32-imported`.
- **Import replace.** Read `PASS replace all clears the old blocks and writes the folder's`. Shot
  `32b-replaced`. The old block list must be empty.
- **Restore.** Read `PASS restore swaps in the chosen graph` and
  `PASS the previous database was saved as a pre-restore copy`. Shot `33-restored`.
- **Proof.** Keep shots `30-settings`, `31-exported`, and `32b-replaced`, plus the restore detail
  naming the pre-restore copy.

## Gotchas

- The file and folder pickers are OS-native and cannot be driven headlessly. The harness opens the
  app-owned confirmation dialogs by object name and calls the action directly.
- The export folder and the import fixture live inside the livetest temp directory. Never point an
  export, import, backup, or restore at the real graph.
- Replace clears the old graph before writing. A failed import after that leaves an empty journal,
  so read the label's `new=[...]` and `old=[...]` values.
- Restore writes a pre-restore copy beside the database. It is the safety net; do not delete it
  during verification.
- Backup names carry a timestamp. Compare counts, not exact file names.
- The settings page has no visible close control. Escape, or the harness `close_settings` hook.
