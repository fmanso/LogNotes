import QtQuick
import QtQuick.Controls
import QtQuick.Window
import com.lognotes.app 1.0

ApplicationWindow {
    id: root
    objectName: "mainWindow"
    width: 820
    height: 900
    visible: true
    title: qsTr("LogNotes")
    color: "#faf9f7"

    // Where the caret should land once the model has been rebuilt. Every save returns a fresh
    // snapshot and resets the model, so the delegate that had focus is gone by the time the rows
    // come back; these carry the caret across that rebuild.
    property int focusRow: -1
    property int focusCursor: 0
    property bool wantsFocus: false

    // Set while a command is in flight. Every command answers with a snapshot, and that snapshot
    // destroys the focused delegate — so the blur that follows must not queue another save, or each
    // save would reset the model again and the caret would never come back.
    property bool rebuilding: false
    property int focusRetries: 0

    // Text typed but not yet written, keyed by row. The debounce would otherwise lose it when the
    // window closes: the process exits before the timer fires.
    property var unsaved: ({})

    // The unlinked-mentions group is collapsed until you ask for it; 0.x behaviour, and it keeps a
    // page's reference panel short.
    property bool unlinkedExpanded: false
    // Switching views replaces every row, so the caret has to be placed somewhere afterwards.
    property bool pendingViewFocus: false
    property var pageList: []

    function refreshPages() {
        try {
            root.pageList = JSON.parse(journal.page_names())
        } catch (error) {
            root.pageList = []
        }
    }

    function flushUnsaved() {
        for (const row in root.unsaved) {
            journal.save_now(Number(row), root.unsaved[row])
        }
        root.unsaved = ({})
    }

    onClosing: function (close) {
        root.flushUnsaved()
        close.accepted = true
    }

    Journal {
        id: journal
    }

    Component.onCompleted: journal.start()

    // The app opens ready to type: today's empty block has the caret before the first keystroke.
    // `revision` fires on every applied snapshot, which `countChanged` cannot do — editing a block
    // leaves the row count untouched.
    Connections {
        target: journal

        function onReadyChanged() {
            if (journal.ready && root.focusRow < 0) {
                root.focusRow = root.firstBlockRow()
                root.focusCursor = 0
                root.wantsFocus = true
                root.focusRetries = 0
                Qt.callLater(root.focusBlock, root.focusRow)
            }
        }

        function onRevisionChanged() {
            root.rebuilding = false
            root.refreshPages()
            if (root.pendingViewFocus) {
                root.pendingViewFocus = false
                root.focusRow = root.firstBlockRow()
                root.focusCursor = 0
                root.wantsFocus = true
                root.focusRetries = 0
                Qt.callLater(root.focusBlock, root.focusRow)
            }
            // Every applied snapshot means the pending text has been written.
            root.unsaved = ({})
            if (root.wantsFocus && root.focusRow >= 0) {
                root.focusRetries = 0
                Qt.callLater(root.focusBlock, root.focusRow)
            }
        }
    }

    Shortcut {
        sequences: ["Ctrl+Z"]
        onActivated: {
            root.rebuilding = true
            journal.undo()
        }
    }

    Shortcut {
        sequences: ["Ctrl+Shift+Z"]
        onActivated: {
            root.rebuilding = true
            journal.redo()
        }
    }

    function firstBlockRow() {
        for (let i = 0; i < list.count; i++) {
            const item = list.itemAtIndex(i)
            if (item && item.isBlock) {
                return i
            }
        }
        return 1
    }

    function focusBlock(row) {
        if (row < 0 || row >= list.count) {
            return false
        }
        root.focusRow = row
        root.wantsFocus = true
        list.currentIndex = row
        list.positionViewAtIndex(row, ListView.Contain)

        const item = list.itemAtIndex(row)
        if (item && item.blockEditor && !item.isHeading) {
            item.blockEditor.forceActiveFocus()
            root.focusCursor = Math.min(root.focusCursor, item.blockEditor.length)
            item.blockEditor.cursorPosition = root.focusCursor
            return true
        }

        // The delegate for this row is not instantiated yet; try again on the next frame.
        if (root.focusRetries < 8) {
            root.focusRetries += 1
            focusRetry.restart()
        }
        return false
    }

    Timer {
        id: focusRetry
        interval: 16
        onTriggered: root.focusBlock(root.focusRow)
    }

    function markerColour(marker) {
        if (marker === "TODO") return "#b07d3a"
        if (marker === "DOING") return "#3a6ea5"
        if (marker === "DONE") return "#4a7c59"
        return "#e4e0d8"
    }

    function formatDay(iso) {
        if (!iso) return ""
        return Qt.formatDate(new Date(iso + "T12:00:00"), "MMM d")
    }

    // One place decides what following a link means: a page link opens that page, anything else leaves
    // the app. Page links and tags share the `page:` scheme, so they behave the same everywhere.
    function openLink(href) {
        if (href.indexOf("page:") === 0) {
            root.unlinkedExpanded = false
            root.focusRow = -1
            root.pendingViewFocus = true
            root.rebuilding = true
            journal.open_page(href.substring("page:".length))
        } else if (href !== "") {
            Qt.openUrlExternally(href)
        }
    }

    // Picking a date is one popup, one decision, Esc always backs out. Qt 6.12 ships Controls'
    // `Calendar` as a singleton that cannot be instantiated, so this is built from MonthGrid.
    Popup {
        id: datePopup
        property int row: -1
        property date monthAnchor: new Date(new Date().getFullYear(), new Date().getMonth(), 1)
        property date picked: new Date()

        parent: Overlay.overlay
        anchors.centerIn: Overlay.overlay
        modal: true
        focus: true
        padding: 12

        function openFor(rowIndex) {
            row = rowIndex
            picked = new Date()
            monthAnchor = new Date(picked.getFullYear(), picked.getMonth(), 1)
            open()
        }

        function shift(days) {
            picked = new Date(picked.getTime() + days * 86400000)
            monthAnchor = new Date(picked.getFullYear(), picked.getMonth(), 1)
        }

        function accept() {
            if (row >= 0) {
                root.rebuilding = true
                journal.set_scheduled(row, Qt.formatDate(picked, "yyyy-MM-dd"))
            }
            close()
        }

        onOpened: popupContent.forceActiveFocus()

        Column {
            id: popupContent
            focus: true
            spacing: 8

            Keys.onPressed: (event) => {
                if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                    datePopup.accept()
                    event.accepted = true
                } else if (event.key === Qt.Key_Escape) {
                    datePopup.close()
                    event.accepted = true
                } else if (event.key === Qt.Key_Left) {
                    datePopup.shift(-1)
                    event.accepted = true
                } else if (event.key === Qt.Key_Right) {
                    datePopup.shift(1)
                    event.accepted = true
                } else if (event.key === Qt.Key_Up) {
                    datePopup.shift(-7)
                    event.accepted = true
                } else if (event.key === Qt.Key_Down) {
                    datePopup.shift(7)
                    event.accepted = true
                }
            }

            Row {
                spacing: 6

                Button {
                    text: "\u2039"
                    onClicked: datePopup.monthAnchor = new Date(datePopup.monthAnchor.getFullYear(),
                                                              datePopup.monthAnchor.getMonth() - 1, 1)
                }

                Label {
                    text: Qt.formatDate(datePopup.monthAnchor, "MMMM yyyy")
                    width: 150
                    horizontalAlignment: Text.AlignHCenter
                    anchors.verticalCenter: parent.verticalCenter
                    color: "#242424"
                }

                Button {
                    text: "\u203a"
                    onClicked: datePopup.monthAnchor = new Date(datePopup.monthAnchor.getFullYear(),
                                                              datePopup.monthAnchor.getMonth() + 1, 1)
                }
            }

            DayOfWeekRow {
                locale: grid.locale
            }

            MonthGrid {
                id: grid
                month: datePopup.monthAnchor
                onClicked: (date) => datePopup.picked = date
            }

            Label {
                text: qsTr("Picked: %1").arg(Qt.formatDate(datePopup.picked, "ddd, MMM d"))
                color: "#5b5b5b"
                font.pixelSize: 12
            }

            Row {
                spacing: 8
                anchors.right: parent.right

                Button {
                    text: qsTr("Clear")
                    onClicked: {
                        root.rebuilding = true
                        journal.set_scheduled(datePopup.row, "")
                        datePopup.close()
                    }
                }

                Button {
                    text: qsTr("Today")
                    onClicked: {
                        datePopup.picked = new Date()
                        datePopup.monthAnchor = new Date(datePopup.picked.getFullYear(),
                                                         datePopup.picked.getMonth(), 1)
                    }
                }

                Button {
                    text: qsTr("OK")
                    onClicked: datePopup.accept()
                }
            }
        }

    }

    // A slim bar: what you are looking at, and the way to something else.
    Rectangle {
        id: topBar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 40
        color: "#f2efe9"

        Row {
            anchors.left: parent.left
            anchors.leftMargin: 28
            anchors.verticalCenter: parent.verticalCenter
            spacing: 10

            Button {
                objectName: "pagesButton"
                text: qsTr("Pages")
                onClicked: pagesPopup.open()
            }
        }
    }

    Popup {
        id: pagesPopup
        parent: Overlay.overlay
        x: 28
        y: 44
        width: 320
        height: Math.min(400, 44 + (root.pageList.length + 1) * 30)
        modal: false
        focus: true

        contentItem: Column {
            spacing: 0
            focus: true

            Rectangle {
                objectName: "pageEntry"
                width: pagesPopup.width
                height: 30
                color: "transparent"

                Label {
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.left: parent.left
                    anchors.leftMargin: 8
                    text: qsTr("Journal")
                    color: "#242424"
                }

                MouseArea {
                    anchors.fill: parent
                    onClicked: {
                        root.unlinkedExpanded = false
                        root.focusRow = -1
                        root.pendingViewFocus = true
                        root.rebuilding = true
                        journal.show_journal()
                        pagesPopup.close()
                    }
                }
            }

            Repeater {
                model: root.pageList

                Rectangle {
                    objectName: "pageEntry"
                    width: pagesPopup.width
                    height: 30
                    color: "transparent"

                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: 8
                        text: modelData
                        color: "#242424"
                        elide: Text.ElideRight
                        width: parent.width - 24
                    }

                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            root.unlinkedExpanded = false
                            root.focusRow = -1
                            root.pendingViewFocus = true
                            root.rebuilding = true
                            journal.open_page(modelData)
                            pagesPopup.close()
                        }
                    }
                }
            }
        }
    }

    ListView {
        id: list
        objectName: "column"
        anchors.fill: parent
        anchors.margins: 28
        anchors.topMargin: 56
        model: journal
        spacing: 3
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        visible: journal.ready && journal.error === ""

        ScrollBar.vertical: ScrollBar {}

        // Days load lazily backwards: reaching the end of what is loaded pulls in a chunk older.
        onAtYEndChanged: {
            if (atYEnd && count > 0) {
                root.rebuilding = true
                journal.load_more()
            }
        }

        delegate: Item {
            id: rowItem
            width: list.width
            height: rowItem.isCollapsedMention
                    ? 0
                    : (rowItem.isHeading
                       ? sectionHeading.implicitHeight + 22
                       : blockRow.height + rowItem.diagramSpace)

            property alias blockEditor: editor
            property bool isBlock: kind === "block" || kind === "placeholder"
            property bool isHeading: kind === "heading" || kind === "agenda-header"
                    || kind === "page-header" || kind === "reference-header"
                    || kind === "unlinked-header"
            property bool isAgenda: kind === "agenda-row"
            property bool isReference: kind === "reference-row" || kind === "unlinked-row"
            property bool isCollapsedMention: kind === "unlinked-row" && !root.unlinkedExpanded
            property bool hovered: false
            // A block's diagram is drawn under its line, so the row has to grow to hold it.
            property real diagramSpace: diagramArea.visible ? diagramArea.implicitHeight + 8 : 0

            HoverHandler {
                onHoveredChanged: rowItem.hovered = hovered
            }

            // A date heading, or the "Scheduled" section heading.
            Label {
                id: sectionHeading
                objectName: kind === "unlinked-header" ? "unlinkedToggle" : ""
                visible: rowItem.isHeading
                anchors.left: parent.left
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 4
                text: kind === "heading"
                      ? Qt.formatDate(new Date(model.date + "T12:00:00"), "dddd, MMMM d, yyyy")
                      : model.text
                color: kind === "page-header" ? "#242424" : "#5b5b5b"
                font.pixelSize: kind === "heading" ? 15 : (kind === "page-header" ? 22 : 13)
                font.bold: true
                font.capitalization: (kind === "heading" || kind === "page-header")
                                     ? Font.MixedCase : Font.AllUppercase

                // The mentions group is the one heading you can click.
                MouseArea {
                    anchors.fill: parent
                    enabled: kind === "unlinked-header"
                    onClicked: root.unlinkedExpanded = !root.unlinkedExpanded
                }
            }

            // Anchored layout rather than a Row: an editor whose width is computed from its own
            // x inside a Row is a binding loop, and Qt resolves it by collapsing the width to 0.
            Item {
                id: blockRow
                visible: !rowItem.isHeading && !rowItem.isCollapsedMention
                width: parent.width
                height: Math.max(editor.visible ? editor.implicitHeight : 0, 24)

                Item {
                    id: indentSpacer
                    width: model.depth * 24
                    height: 1
                }

                // The marker box is both the affordance and the state.
                Rectangle {
                    id: markerBox
                    objectName: rowItem.isAgenda ? "agendaCheck" : "blockCheck"
                    // Only worth drawing when it says something, or when the pointer is near it.
                    visible: model.marker !== "" || rowItem.hovered || editor.activeFocus
                    x: indentSpacer.width + 4
                    width: 14
                    height: 14
                    radius: 3
                    anchors.verticalCenter: parent.verticalCenter
                    color: model.marker === "" ? "transparent" : root.markerColour(model.marker)
                    border.color: root.markerColour(model.marker)
                    border.width: 1

                    Label {
                        anchors.centerIn: parent
                        visible: model.marker === "DONE"
                        text: "\u2713"
                        color: "#ffffff"
                        font.pixelSize: 10
                        font.bold: true
                    }

                    MouseArea {
                        anchors.fill: parent
                        acceptedButtons: Qt.LeftButton
                        onClicked: {
                            root.rebuilding = true
                            if (rowItem.isAgenda) {
                                // Checking something off means "done", not "next marker along".
                                journal.set_marker(index, "DONE")
                            } else {
                                journal.cycle_marker(index)
                            }
                        }
                    }
                }

                Label {
                    id: markerLabel
                    x: markerBox.x + markerBox.width + 4
                    visible: model.marker !== ""
                    text: model.marker
                    color: root.markerColour(model.marker)
                    font.pixelSize: 11
                    font.bold: true
                    anchors.verticalCenter: parent.verticalCenter
                }

                Label {
                    id: scheduledChip
                    visible: !rowItem.isAgenda && model.scheduled !== ""
                    text: formatDay(model.scheduled)
                    color: model.overdue ? "#a4443c" : "#8a857c"
                    font.pixelSize: 12
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                }

                TextEdit {
                    id: editor
                    objectName: "blockEditor"
                    visible: rowItem.isBlock
                    anchors.left: markerLabel.visible ? markerLabel.right : markerBox.right
                    anchors.leftMargin: 8
                    anchors.right: scheduledChip.visible ? scheduledChip.left : parent.right
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    text: model.text
                    color: model.marker === "DONE" ? "#8a857c" : "#242424"
                    font.pixelSize: 16
                    font.strikeout: model.marker === "DONE"
                    wrapMode: TextEdit.Wrap
                    selectByMouse: true
                    activeFocusOnPress: true

                    // Styling lives on this editor's own document, so the caret, the undo stack and the
                    // markdown that gets saved are untouched by it.
                    Component.onCompleted: journal.attach_highlighter(editor.textDocument)

                    // A block is always in edit mode, so a plain click has to keep meaning "put the
                    // caret here": only Ctrl+click navigates.
                    TapHandler {
                        acceptedModifiers: Qt.ControlModifier
                        onSingleTapped: (event) => {
                            const position = editor.positionAt(event.position.x, event.position.y)
                            root.openLink(journal.link_at(editor.text, position))
                        }
                    }

                    // Restores the caret when the model rebuild hands focus back, and flushes a
                    // pending debounced save when the block is left.
                    onActiveFocusChanged: {
                        if (activeFocus) {
                            if (root.wantsFocus) {
                                cursorPosition = Math.min(root.focusCursor, length)
                            }
                        } else if (debounce.running && !root.rebuilding) {
                            debounce.stop()
                            root.rebuilding = true
                            journal.edit(index, editor.text)
                        }
                    }

                    // Typing is debounced; structural keys flush immediately.
                    Timer {
                        id: debounce
                        interval: 250
                        onTriggered: {
                            root.rebuilding = true
                            journal.edit(index, editor.text)
                        }
                    }

                    onTextEdited: {
                        root.focusRow = index
                        root.focusCursor = cursorPosition
                        root.wantsFocus = true
                        root.unsaved = Object.assign({}, root.unsaved, { [index]: editor.text })
                        debounce.restart()
                    }

                    Keys.onPressed: (event) => {
                        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                            debounce.stop()
                            if (event.modifiers & Qt.ShiftModifier) {
                                // A line break inside the block: a fence or a diagram needs one, while
                                // plain Enter must keep meaning "new block".
                                editor.insert(editor.cursorPosition, "\n")
                                root.focusRow = index
                                root.focusCursor = editor.cursorPosition
                                root.wantsFocus = true
                                root.unsaved = Object.assign({}, root.unsaved, { [index]: editor.text })
                                debounce.restart()
                            } else {
                                root.rebuilding = true
                                if (event.modifiers & Qt.ControlModifier) {
                                    // Ctrl+Enter marks the block; plain Enter splits it.
                                    journal.cycle_marker(index)
                                } else {
                                    root.focusRow = index + 1
                                    root.focusCursor = 0
                                    root.wantsFocus = true
                                    journal.submit_block(index, cursorPosition, text)
                                }
                            }
                            event.accepted = true
                        } else if (event.key === Qt.Key_S
                                   && (event.modifiers & Qt.ControlModifier)
                                   && (event.modifiers & Qt.ShiftModifier)) {
                            datePopup.openFor(index)
                            event.accepted = true
                        } else if (event.key === Qt.Key_Tab) {
                            debounce.stop()
                            root.rebuilding = true
                            journal.edit(index, text)
                            journal.indent(index)
                            event.accepted = true
                        } else if (event.key === Qt.Key_Backtab) {
                            debounce.stop()
                            root.rebuilding = true
                            journal.edit(index, text)
                            journal.outdent(index)
                            event.accepted = true
                        } else if (event.key === Qt.Key_Backspace && text.length === 0) {
                            // Backspace on an empty block removes it; on a day's first block the
                            // core refuses to merge across days, so nothing is lost.
                            debounce.stop()
                            if (!model.placeholder) {
                                root.rebuilding = true
                                journal.remove(index)
                            }
                            event.accepted = true
                        } else if (event.key === Qt.Key_Z && (event.modifiers & Qt.ControlModifier)) {
                            // TextEdit would swallow Ctrl+Z for its own per-widget undo, which knows
                            // nothing about block creation, indentation, or other days.
                            debounce.stop()
                            root.rebuilding = true
                            if (text !== model.text) {
                                journal.edit(index, text)
                            }
                            if (event.modifiers & Qt.ShiftModifier) {
                                journal.redo()
                            } else {
                                journal.undo()
                            }
                            event.accepted = true
                        } else if (event.key === Qt.Key_Up) {
                            root.focusCursor = 0
                            root.focusBlock(index - 1)
                            event.accepted = true
                        } else if (event.key === Qt.Key_Down) {
                            root.focusCursor = 0
                            root.focusBlock(index + 1)
                            event.accepted = true
                        }
                    }
                }

                // An agenda row is the same block seen from today: read it, or check it off here.
                Label {
                    id: agendaText
                    objectName: "summaryRow"
                    visible: rowItem.isAgenda || rowItem.isReference
                    anchors.left: markerLabel.visible ? markerLabel.right : markerBox.right
                    anchors.leftMargin: 8
                    anchors.right: sourceLabel.left
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    // Summary rows are not edited, so they show the finished text: marks dropped and
                    // links live.
                    text: model.html === "" ? qsTr("(empty block)") : model.html
                    textFormat: Text.RichText
                    onLinkActivated: (link) => root.openLink(link)
                    color: model.overdue ? "#a4443c" : "#242424"
                    font.pixelSize: 16
                    elide: Text.ElideRight
                }

                Label {
                    id: sourceLabel
                    visible: rowItem.isAgenda || rowItem.isReference
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    text: {
                        if (rowItem.isReference) {
                            const isDate = /^\d{4}-\d{2}-\d{2}$/.test(model.source)
                            return isDate ? "from " + formatDay(model.source) : "from " + model.source
                        }
                        if (!rowItem.isAgenda) {
                            return ""
                        }
                        const day = formatDay(model.scheduled)
                        const isDate = /^\d{4}-\d{2}-\d{2}$/.test(model.source)
                        if (model.source === "" || model.source === model.scheduled) {
                            return day
                        }
                        return day + " · from " + (isDate ? formatDay(model.source) : model.source)
                    }
                    color: model.overdue ? "#a4443c" : "#8a857c"
                    font.pixelSize: 12
                }
            }

            // A ```mermaid fence draws its diagram here, under the fence itself. A failed diagram keeps
            // the fence as code and says why in one quiet line.
            Column {
                id: diagramArea
                visible: (model.diagram !== "" || model.diagramError !== "")
                         && !rowItem.isHeading && !rowItem.isCollapsedMention
                anchors.top: blockRow.bottom
                anchors.topMargin: 8
                // A sibling, not the editor: an anchor target has to be a parent or a sibling.
                anchors.left: blockRow.left
                anchors.leftMargin: editor.x
                anchors.right: blockRow.right
                spacing: 6

                Image {
                    objectName: "diagram"
                    visible: model.diagram !== ""
                    source: model.diagram
                    // Never blow a small diagram up to the column's width; only the wide ones shrink,
                    // and a very tall one is capped so it cannot swallow the page.
                    width: Math.min(implicitWidth, diagramArea.width)
                    height: implicitWidth > 0
                            ? Math.min(width * implicitHeight / implicitWidth, 420)
                            : 0
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                }

                Label {
                    objectName: "diagramError"
                    visible: model.diagramError !== ""
                    text: model.diagramError
                    color: "#a4443c"
                    font.pixelSize: 12
                    width: parent.width
                    wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                }
            }
        }
    }

    Rectangle {
        id: errorPane
        anchors.fill: parent
        color: "#faf9f7"
        visible: !journal.ready && journal.error !== ""

        Column {
            anchors.centerIn: parent
            spacing: 14
            width: Math.min(parent.width - 80, 620)

            Label {
                text: qsTr("LogNotes could not open its database.")
                color: "#242424"
                font.pixelSize: 18
                font.bold: true
            }

            Label {
                width: parent.width
                text: journal.error
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                color: "#7a3a3a"
            }

            Label {
                width: parent.width
                text: qsTr("Your notes are still on disk; LogNotes just cannot open them right now.")
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                color: "#5b5b5b"
            }

            Row {
                spacing: 10

                Button {
                    text: qsTr("Retry")
                    onClicked: journal.start()
                }

                Button {
                    text: qsTr("Quit")
                    onClicked: Qt.quit()
                }
            }
        }
    }

    Label {
        visible: journal.ready && journal.error !== ""
        anchors.bottom: parent.bottom
        anchors.right: parent.right
        anchors.margins: 12
        text: journal.error
        color: "#7a3a3a"
    }
}
