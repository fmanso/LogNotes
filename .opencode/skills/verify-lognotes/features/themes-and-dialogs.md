# Themes and dialogs

The theme button switches the whole window between light and dark, and the choice persists. Every
app-owned modal dialog follows the current theme, including rename, import, and export, and it
re-themes while open.

## Sub-features

- `theme-toggle` switches light and dark from the theme button.
- `theme-dialog-rename` opens the rename dialog in the current theme and follows a switch.
- `theme-dialog-import` opens the import mode dialog in the current theme and follows a switch.
- `theme-dialog-export` opens the export confirmation in the current theme and follows a switch.
- `theme-dialog-font` opens the note font picker in the current theme and follows a switch.
- `theme-settings` keeps the settings page legible across a switch.

## How to get to it (user POV)

- Click the sun or moon button in the header.
- Open a page menu and choose Rename.
- In settings, choose Export or Import.
- Watch the dialog stay legible while the theme changes.

## Driving it with livetest

Preconditions:

- `scripts/doctor.sh` passes.
- `scripts/verify.sh` has produced `livetest.log` and `shots/`.
- The page steps ran first, so the rename dialog has content.

- **Toggle.** Read `PASS the theme button switches to dark: theme=dark` and
  `PASS and back to light: theme=light`. Shot `16b-dark-theme`.
- **Rename dialog.** Read `PASS the rename dialog opens in the light theme: theme=light` and
  `PASS the rename dialog follows the theme switch: theme=dark`. Shots
  `29a-rename-dialog-light`, `29a-rename-dialog-dark`.
- **Settings.** Read `PASS the settings page survives the theme switch: theme=dark`. Shot
  `31b-settings-dark`.
- **Font picker.** Read `PASS the font picker follows the theme switch: theme=dark`. Shots
  `30a-font-dialog-light`, `30b-font-dialog-dark`. The dialog must stay open across the switch.
- **Import dialog.** Read `PASS the import mode dialog opens in the light theme` and
  `PASS the import mode dialog follows the theme switch`. Shots `31c-import-dialog-light`,
  `31d-import-dialog-dark`.
- **Export dialog.** Read `PASS the export confirmation opens in the light theme` and
  `PASS the export confirmation follows the theme switch`. Shots `31e-export-dialog-light`,
  `31f-export-dialog-dark`.
- **Proof.** Keep one light and one dark shot of the same dialog, plus the log lines. The pair is the
  proof that the dialog follows the theme.

## Gotchas

- The theme is a stored setting, not an environment variable. It persists across launches.
- The OS-native file and folder pickers are not themed by the app. Do not judge them against the
  app-owned dialogs.
- The dialog must stay open across the theme switch. Opening it twice proves less.
- Legibility is visual. Read the screenshot, and compare body text and title against the panel.
- The harness switches the theme mid-dialog. A dialog that only reads the theme at open time fails
  the "follows the theme switch" label.
