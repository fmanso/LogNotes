# Spec Delta

## Purpose

Makes every in-app modal dialog read as part of the window in both the light and dark themes, regardless
of the system palette, while keeping the app's existing dialog interactions.

## ADDED Requirements

### Requirement: Dialogs follow the active theme
Every in-app modal dialog SHALL draw its panel, title, body text, dimming overlay, and controls from the
active light/dark theme rather than from the system palette. A dialog SHALL remain legible when the system
palette and the app theme disagree.

#### Scenario: Light theme
- **WHEN** the app theme is light and a dialog opens
- **THEN** its panel is drawn in the light theme's surface colours with dark theme text and its overlay
  dims without making the dialog unreadable

#### Scenario: Dark theme
- **WHEN** the app theme is dark and a dialog opens
- **THEN** its panel is drawn in the dark theme's surface colours with light theme text and its controls
  remain readable

#### Scenario: System palette disagrees
- **WHEN** the desktop palette is dark while the app theme is light, or the reverse
- **THEN** the dialog still follows the app theme, not the desktop palette

### Requirement: Dialogs share the window's surface and controls
In-app dialogs SHALL use the same visual vocabulary as the window's other overlays: the themed page
surface, a hairline border, rounded corners, themed hover states, and the window's quiet buttons for their
actions. The rename, delete, move-to-page, import-mode, restore-confirmation, and export-confirmation
dialogs SHALL all use that presentation.

#### Scenario: One presentation across dialogs
- **WHEN** any of the rename, delete, move-to-page, import-mode, restore, or export dialogs opens
- **THEN** it shows the same themed panel, border, title treatment, and button style as the app's date
  picker and menus

### Requirement: Editable dialog fields stay legible in both themes
Text inputs inside dialogs SHALL use the active theme for their surface, text, border, placeholder, and
focused border, including the rename field and the move-to-page filter.

#### Scenario: Rename field in either theme
- **WHEN** the rename dialog opens in the light or dark theme
- **THEN** the current name is readable in the theme's text colour, the field border is the theme's
  border, and the focused field uses the theme's accent

#### Scenario: Move filter in either theme
- **WHEN** the user types into the move-to-page filter in the light or dark theme
- **THEN** both the filter text and its placeholder are readable against the dialog surface

### Requirement: Theming does not change dialog interaction
Restyling a dialog SHALL NOT change how it is operated: the appropriate field or list SHALL take focus on
open, Enter SHALL accept, Escape SHALL cancel or dismiss without writing, and the move-to-page list SHALL
keep keyboard navigation.

#### Scenario: Rename keeps its keyboard path
- **WHEN** the rename dialog opens and the user edits the name and presses Enter
- **THEN** the rename is submitted exactly as before

#### Scenario: Escape backs out
- **WHEN** any of these dialogs is open and the user presses Escape
- **THEN** the dialog closes without performing its action

#### Scenario: Move picker keyboard path
- **WHEN** the move-to-page dialog is open
- **THEN** the filter takes focus, arrow keys move through the page list, and Enter moves the block to the
  highlighted page
