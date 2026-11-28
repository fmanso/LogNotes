# Design

## Context

See `proposal.md` for the motivation and `specs/dialog-presentation/spec.md` for the requirements.

The window already has one palette: a `QtObject` named `theme` with roles such as `page`, `panel`,
`border`, `text`, `textMuted`, `textGhost`, `hover`, `accent`, and `scrim`, all computed from
`root.darkTheme`. `QuietButton` is the window's button, and the slash menu, link completion, page menu,
and date picker each give their `Popup` the same skinned background: `theme.page`, a `theme.border`
hairline, and an 8-point radius.

The six app-owned dialogs are stock `QtQuick.Controls.Dialog` instances. The Basic style gives them a
`palette.window` background, a `palette.windowText` header, a `palette.shadow` modal overlay, and stock
`TextField` chrome. Those come from the system palette, which is independent of the app theme. The dialogs
also inherit the style's square panel and header treatment rather than the app's popup surface. The native
`FileDialog` and `FolderDialog` are separate platform windows and are out of scope.

## Goals / Non-Goals

**Goals:**

- One re-usable on-screen presentation for every in-app modal dialog, driven entirely by `theme`.
- Text fields inside dialogs that are as legible and as obviously themed as the window's search field.
- No change to the ids, object names, focus order, keyboard paths, or actions the live test drives.

**Non-Goals:**

- Replacing `Dialog` with `Popup`, or re-implementing modal focus and dismissal.
- Theming the OS-native file and folder pickers.
- Adding a new theme, a new palette, or a general restyle of non-dialog controls.
- Changing any core, store, or worker behaviour.

## Decisions

### Keep `Dialog`, skin it with one inline component

Add an inline `ThemedDialog` component beside `QuietButton`, and change the six instances to it:

```qml
component ThemedDialog: Dialog {
    id: control
    anchors.centerIn: Overlay.overlay
    modal: true
    padding: 12

    background: Rectangle {
        color: theme.page
        border.color: theme.border
        radius: 8
    }

    header: Label {
        visible: control.title !== ""
        text: control.title
        color: theme.text
        font.pixelSize: 14
        font.bold: true
        topPadding: 8
        bottomPadding: 0
        leftPadding: 12
        rightPadding: 12
    }

    Overlay.modal: Rectangle {
        color: theme.scrim
        opacity: 0.35
    }
}
```

`Dialog` stays because its modal overlay, focus handling, `closePolicy`, and `title` semantics are what the
dialogs already rely on; only its visual chrome changes. A `Popup` would mean re-creating the modal
behaviour for no gain.

The custom header replaces the style header because the stock one paints `palette.windowText` over
`palette.window`; the label keeps the `title` property as the single source of the text. It draws no
background, so the panel's rounded surface shows through and the header cannot paint square corners over
the radius. The header carries its own top and side padding: the Dialog lays the header out at the panel
edge, unlike its content.

Confirmation dialogs whose `contentItem` is a wrapped `Label` also set an explicit dialog `width`. A
wrapped `Label`'s `implicitWidth` is the natural width of its longest line, so leaving the Dialog to size
itself makes the panel as wide as the window as soon as the text is long. The widths here are the content
width plus the dialog's padding: 424 for the 400-point import and delete labels, 404 for restore's 380,
and 384 for export's 360.

**Alternatives considered:** setting `ApplicationWindow.palette` from the theme would recolour the stock
chrome, but it would still leave square, stock-looking panels and would silently restyle every other
control that reads the palette. Styling each dialog ad hoc would work today and drift tomorrow; the change
is exactly the moment to make the presentation shared.

### One themed field for dialog inputs

Add an inline `QuietField` beside `QuietButton`, modelled on the search field's surface:

```qml
component QuietField: TextField {
    id: control
    color: theme.text
    placeholderTextColor: theme.textGhost
    selectionColor: theme.accent
    selectedTextColor: theme.page
    leftPadding: 10
    rightPadding: 10
    topPadding: 7
    bottomPadding: 7

    background: Rectangle {
        color: theme.page
        border.color: control.activeFocus ? theme.accent : theme.border
        radius: 6
    }
}
```

Use it for the rename field and the move-to-page filter. The search field has the same visual rule, so a
dialog input now reads as the same kind of control as the bar's field.

**Alternative considered:** relying on the control palette for the fields. Rejected because the palette
does not carry the app's accent or placeholder roles, and the focused border would not follow the theme.

### Keep content and footers in the instances

The dialog-specific parts stay where they are: the rename dialog's `QuietField`, the move picker's filter
and `ListView`, and each confirmation's `Label` plus row of `QuietButton`s. A shared `contentItem` would
have to become a slot with a new API; the shared part is the surface, not the contents.

The move list already uses `theme.hover` for its current row and `theme.text` for its labels, so it needs
no change beyond the filter field. The confirmation labels already use `theme.text`.

### Verify through the live test

The live test already drives rename, delete, and move-to-page through the real objects. Add screenshots
while a dialog is open in the light theme and again in the dark theme, and keep the existing action checks.
The import-mode and export-confirmation dialogs normally follow a native folder picker that cannot be
driven headlessly, so a driver helper opens their QML dialog objects by name for the screenshots and the
test cancels them without writing. This is the same evidence path the project uses for the date picker and
the settings page: a PNG a human can read, plus the action that proves the dialog still works.

The dialog's surface colour is not asserted numerically. A property reader would test a colour literal,
not legibility, and the screenshot is what the project already treats as the visual check. The switch
itself is observable through `journal.theme`, so the scenario can assert the dialog is open while the
theme is dark and still completes the action.

## Risks / Trade-offs

- **A custom header changes implicit sizing and could push a dialog off-centre or reduce content height**
  → keep the header a single-line `Label` with no extra padding and verify each dialog's open geometry in
  the live run at the window's default size.
- **`Overlay.modal` on the shared component may be overridden or ignored if an instance sets its own
  overlay** → no instance does; if a future dialog needs a different dim, it can set the property on that
  instance explicitly.
- **Inline components share the file's scope and the new component references `theme`** → the existing
  `QuietButton` already does the same, so this follows the file's established pattern.
- **The title may be invisible on very narrow dialogs** → all six titles are short and each content item
  already sets a width between 360 and 420 points; the header elides rather than clips.
- **The field's accent border is the only focus cue** → the dialog still puts the caret in the field, and
  the border colour change plus the caret is the same cue the bar's search field uses.

## Migration Plan

No data, schema, or process migration. Replace the six `Dialog` instances with `ThemedDialog`, replace the
two `TextField`s with `QuietField`, keep every id and object name, then update `lognotes-qt/SPEC.md`. Roll
back is the reverse edit; no persisted state depends on the presentation.

## Open Questions

None.
