# Tasks

## 1. Shared dialog presentation

- [x] 1.1 Add the `ThemedDialog` inline component beside `QuietButton` in `lognotes-qt/qml/main.qml`, with
  the themed `background`, custom `header`, and `Overlay.modal` from design.md; verify the file still loads
  with `make build`.
- [x] 1.2 Add the `QuietField` inline `TextField` beside `QuietButton`, using `theme.text`,
  `theme.textGhost`, `theme.border`, `theme.accent`, and `theme.page` as described in design.md; verify the
  app builds and the bar's search field still works in `make livetest`.

## 2. Dialog instances

- [x] 2.1 Convert the rename and delete page dialogs to `ThemedDialog` and make the rename `contentItem` a
  `QuietField`; verify the existing rename and delete steps in `make livetest` still pass, including focus,
  Ctrl+A, Enter, Cancel, and the linked-page refusal.
- [x] 2.2 Convert the move-to-page dialog to `ThemedDialog` and its filter to `QuietField`; verify the
  existing move step in `make livetest` still filters, navigates with the arrow keys, and moves the block on
  Enter.
- [x] 2.3 Convert the import-mode, restore-confirmation, and export-confirmation dialogs to
  `ThemedDialog`; verify the existing Settings backup/export/import/restore steps in `make livetest` still
  pass and their object names are unchanged.

## 3. Live visual coverage

- [x] 3.1 Add a live-test step that opens the rename or delete dialog in the light theme, takes a
  screenshot, switches to the dark theme with the dialog open, takes another screenshot, and then completes
  the dialog action; verify `make livetest` reports both steps passed and the PNGs show a themed panel in
  each theme.
- [x] 3.2 Add a live-test step that opens the move-to-page dialog in each theme, screenshots it, and
  completes a move; verify the move lands in the store and the screenshots show readable text and the
  themed field in both themes.
- [x] 3.3 Add screenshots for the import-mode and restore or export confirmation dialogs in the Settings
  flow in both themes; verify `make livetest` completes the flow and the PNGs show the same dialog surface
  as the other dialogs.

## 4. Design record

- [x] 4.1 Update the `lognotes-qt/SPEC.md` view-behaviour section with the rule that in-app modal dialogs
  are skinned from the app theme rather than the system palette, naming why the stock Controls dialog
  chrome is not used; verify the text matches the implemented components and still agrees with
  `openspec/specs/dialog-presentation/spec.md`.

## 5. Integration check

- [x] 5.1 Run `make test` and `make livetest` together; verify every unit test and live step passes, and
  inspect the dialog screenshots in both themes for legibility and consistency with the app's other
  popups.

## Workflow follow-up

- Archive the change after review and after the live screenshots have been checked.
