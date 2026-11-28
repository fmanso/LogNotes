# Proposal

## Why

The app's modal dialogs — rename, delete, move to page, import mode, restore confirmation, export
confirmation — are stock `QtQuick.Controls.Dialog` instances. Their panel, title header, dimming overlay,
and text fields come from the control style and the system palette, not from the app's own light/dark
theme. On a light app theme with a dark system palette, the panel is dark while body labels and
`QuietButton` text use the app's dark-on-light colours, so the dialog is hard to read; in either theme the
dialog's flat stock chrome looks unlike the cream/dark popups used everywhere else. The rest of the window
already keys every colour from one theme object, so these dialogs are the remaining mismatch.

## What Changes

- Give every in-app modal dialog one shared presentation: the window's theme colours for panel, hairline
  border, title, body text, dimming overlay, and hover states, matching the existing date picker and menus.
- Style the two dialog text inputs (rename field and move-to-page filter) with the app's text, border,
  placeholder, and focus-accent colours instead of the stock control chrome.
- Apply that presentation to all six app-owned dialogs: rename page, delete page, move block to page,
  import mode, restore confirmation, and export confirmation.
- Keep the current behaviour, keyboard handling, focus rules, object names, and live-test pathways intact;
  only how the dialogs are drawn and themed changes.
- Leave the OS-native `FileDialog` and `FolderDialog` untouched: their appearance belongs to the platform
  and cannot follow the app theme.
- Update the module design record to state the rule for modal dialog presentation, and exercise the
  dialogs in both themes in the live test with screenshots.

## Capabilities

### New Capabilities

- `dialog-presentation`: how in-app modal dialogs are drawn in the current light/dark theme, so every
  dialog reads as part of the window and stays legible regardless of the system palette.

### Modified Capabilities

<!-- None: rename, delete, and move behaviour is unchanged; this change is presentation only. -->

## Impact

- `lognotes-qt/qml/main.qml`: shared dialog and dialog-field presentation; the six `Dialog` instances.
- `lognotes-qt/SPEC.md`: the dialog presentation rule and the reason stock Controls dialog chrome is not
  used.
- `lognotes-qt/src/livetest.rs`: screenshots and checks covering dialogs in light and dark themes.
- No core, store, schema, API, or dependency changes.
