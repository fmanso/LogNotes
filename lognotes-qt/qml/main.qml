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
            if (root.wantsFocus && root.focusRow >= 0) {
                root.focusRetries = 0
                Qt.callLater(root.focusBlock, root.focusRow)
            }
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

    ListView {
        id: list
        anchors.fill: parent
        anchors.margins: 28
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
            height: kind === "heading" ? heading.implicitHeight + 22 : Math.max(blockRow.implicitHeight, 24)

            property alias blockEditor: editor
            property bool isBlock: kind !== "heading"
            property bool isHeading: kind === "heading"

            Label {
                id: heading
                visible: kind === "heading"
                anchors.left: parent.left
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 4
                text: kind === "heading"
                      ? Qt.formatDate(new Date(model.date + "T12:00:00"), "dddd, MMMM d, yyyy")
                      : ""
                font.pixelSize: 15
                font.bold: true
                color: "#5b5b5b"
            }

            Row {
                id: blockRow
                visible: kind !== "heading"
                anchors.top: parent.top
                width: parent.width
                height: Math.max(editor.implicitHeight, 24)

                Item {
                    width: model.depth * 24
                    height: 1
                }

                Rectangle {
                    width: 5
                    height: 5
                    radius: 2.5
                    color: "#c9c4bb"
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.verticalCenterOffset: 2
                }

                Item {
                    width: 14
                    height: 1
                }

                TextEdit {
                    id: editor
                    objectName: "blockEditor"
                    width: blockRow.width - x - 8
                    anchors.verticalCenter: parent.verticalCenter
                    text: model.text
                    color: "#242424"
                    font.pixelSize: 16
                    wrapMode: TextEdit.Wrap
                    selectByMouse: true
                    activeFocusOnPress: true

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
                        debounce.restart()
                    }

                    Keys.onPressed: (event) => {
                        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                            debounce.stop()
                            // The new block is the next row; the caret follows it.
                            root.focusRow = index + 1
                            root.focusCursor = 0
                            root.wantsFocus = true
                            root.rebuilding = true
                            journal.submit_block(index, cursorPosition, text)
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
                font.pixelSize: 18
                font.bold: true
            }

            Label {
                width: parent.width
                text: journal.error
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                color: "#7a3a3a"
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
