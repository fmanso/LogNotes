import QtQuick
import QtQuick.Controls
import QtQuick.Window
import com.lognotes.app 1.0

ApplicationWindow {
    id: root
    objectName: "mainWindow"
    width: 820
    height: 900
    // Shown from the start and faded in once the store has answered, because the theme lives there:
    // appearing light and then turning dark is worse than a few milliseconds of nothing. `visible` stays
    // true — a window that is created hidden and then shown gets tiled by the compositor, and one that is
    // never shown at all is a window nobody can use.
    visible: true
    opacity: root.revealed ? 1 : 0
    title: qsTr("LogNotes")
    color: theme.page

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

    // Every colour in the window, in one place and keyed by theme: a third theme would be another table
    // of these roles, not a sweep through the file.
    QtObject {
        id: theme
        readonly property color page: root.darkTheme ? "#17181a" : "#faf9f7"
        readonly property color panel: root.darkTheme ? "#1f2124" : "#f2efe9"
        readonly property color rule: root.darkTheme ? "#2c2f33" : "#ece7de"
        readonly property color border: root.darkTheme ? "#34383d" : "#ddd8ce"
        readonly property color text: root.darkTheme ? "#e6e3dc" : "#242424"
        readonly property color textMuted: root.darkTheme ? "#b8b2a6" : "#5b5b5b"
        readonly property color textFaint: root.darkTheme ? "#a39c90" : "#8a857c"
        readonly property color textGhost: root.darkTheme ? "#8f8a80" : "#a39c90"
        readonly property color hover: root.darkTheme ? "#26292d" : "#ece7de"
        readonly property color accent: root.darkTheme ? "#8fb6e8" : "#3a6ea5"
        readonly property color danger: root.darkTheme ? "#e0948a" : "#a4443c"
        readonly property color dangerText: root.darkTheme ? "#e8b0a8" : "#7a3a3a"
        readonly property color success: root.darkTheme ? "#8fc79c" : "#4a7c59"
        readonly property color warning: root.darkTheme ? "#d8b076" : "#b07d3a"
        readonly property color markerIdle: root.darkTheme ? "#3a3f45" : "#e4e0d8"
        readonly property color scrim: root.darkTheme ? "#000000" : "#3c3223"
        readonly property color onMarker: "#ffffff"
    }
    // The theme the store remembered, or light until it says otherwise.
    property bool darkTheme: journal.theme === "dark"

    // The quick switcher: one field over the whole graph. `searchHits` is rebuilt from the model's JSON
    // answer, and `searchMessage` is empty unless there is something to say.
    property int searchSelected: -1
    property string searchMessage: ""
    // Where the block hits start, so the list can label its two groups.
    property int searchFirstBlock: -1

    // The one block currently in edit mode; every other block shows its markdown rendered. It lives on
    // the window rather than on a delegate because delegates are rebuilt after every save.
    property int editingRow: -1
    // A word to put the caret on once a block is focused — the search term a hit was found by. It is
    // applied in `focusBlock`, because the delegate may not exist yet when the hit is activated.
    property string focusTerm: ""
    // The term a search hit was activated with, captured before the field is cleared.
    property string pendingTerm: ""

    // The unlinked-mentions group is collapsed until you ask for it; 0.x behaviour, and it keeps a
    // page's reference panel short.
    property bool unlinkedExpanded: false
    // Switching views replaces every row, so the caret has to be placed somewhere afterwards.
    property bool pendingViewFocus: false
    property bool sidebarOpen: true
    property var sidebarFavourites: []
    property var sidebarRecent: []
    // The window waits for the store, because the theme lives there and appearing light and then
    // turning dark is worse than appearing a few milliseconds later.
    property bool revealed: false
    // Empty past days are hidden, so the column can be shorter than the window. Older chunks are
    // pulled until it fills, or until a few pulls in a row have found nothing.
    property int emptyPulls: 0

    // Reaching a page, wherever the click came from: the sidebar, a link, a search hit.
    function openPage(name) {
        searchField.focus = false
        root.emptyPulls = 0
        root.editingRow = -1
        root.unlinkedExpanded = false
        root.focusRow = -1
        root.pendingViewFocus = true
        root.rebuilding = true
        journal.open_page(name)
    }

    function goJournal() {
        searchField.focus = false
        root.emptyPulls = 0
        root.editingRow = -1
        root.unlinkedExpanded = false
        root.focusRow = -1
        root.pendingViewFocus = true
        root.rebuilding = true
        journal.show_journal()
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

    // Every button in the window: an outlined, quiet control in the app's own palette, instead of the
    // control style's default dark chrome (which read as a different application on a cream page).
    component QuietButton: Button {
        id: control
        background: Rectangle {
            color: control.hovered ? theme.hover : "transparent"
            border.color: theme.border
            radius: 6
        }
        contentItem: Label {
            text: control.text
            color: theme.text
            font.pixelSize: 13
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
    }

    // If the store never answers, show the window anyway rather than leave nothing on screen.
    Timer {
        interval: 2000
        running: !root.revealed
        onTriggered: {
            root.revealed = true
            if (root.focusRow < 0) {
                root.beginEdit(root.firstBlockRow(), 0)
            }
        }
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
            // The ready path reveals in `onRevisionChanged`, after the column has been filled; only the
            // failure path has no snapshot to wait for.
            if (journal.error !== "") {
                root.revealed = true
            }
        }

        function onSidebarChanged() {
            let body = { favourites: [], recent: [] }
            try {
                body = JSON.parse(journal.sidebar)
            } catch (error) {
                body = { favourites: [], recent: [] }
            }
            root.sidebarFavourites = body.favourites || []
            root.sidebarRecent = body.recent || []
        }

        function onHitsChanged() {
            let body = { error: "", hits: [] }
            try {
                body = JSON.parse(journal.hits)
            } catch (error) {
                body = { error: qsTr("The search answer could not be read."), hits: [] }
            }

            searchModel.clear()
            for (const hit of body.hits) {
                searchModel.append(hit)
            }
            root.searchSelected = searchModel.count > 0 ? 0 : -1
            root.searchFirstBlock = -1
            for (let i = 0; i < searchModel.count; i++) {
                if (searchModel.get(i).kind === "block") {
                    root.searchFirstBlock = i
                    break
                }
            }
            root.searchMessage = body.error !== ""
                    ? body.error
                    : (searchModel.count === 0 && searchField.text.trim() !== ""
                       ? qsTr("Nothing matches “%1”").arg(searchField.text) : "")
        }

        function onRevisionChanged() {
            // `rebuilding` stays up for the rest of this turn: the delegate that was just destroyed
            // reports its blur *after* this handler, and that blur must not render the block the
            // caret is about to go back into.
            Qt.callLater(function () { root.rebuilding = false })

            // Empty past days are hidden, so a fresh column can be shorter than the window. Older days
            // are pulled in *before* the window appears — each pull resets the model, and doing that
            // after the caret has been granted loses it.
            if (!root.revealed) {
                if (journal.ready && list.atYEnd && root.emptyPulls < 3 && list.count > 0) {
                    root.emptyPulls += 1
                    root.rebuilding = true
                    journal.load_more()
                    return
                }
                root.revealed = true
            }

            // A search hit asked for the caret to be put in a block: this snapshot has the row, so put
            // the caret on the match rather than at the start of the block.
            if (journal.focus_row >= 0) {
                const row = journal.focus_row
                journal.clear_focus_row()
                root.focusTerm = root.pendingTerm
                root.pendingTerm = ""
                root.beginEdit(row, 0)
            }
            if (root.pendingViewFocus) {
                root.pendingViewFocus = false
                root.beginEdit(root.firstBlockRow(), 0)
            }
            // Every applied snapshot means the pending text has been written.
            root.unsaved = ({})

            if (root.focusRow < 0) {
                // Nothing has asked for the caret yet: the app opens ready to type.
                root.beginEdit(root.firstBlockRow(), 0)
            } else if (root.wantsFocus) {
                root.focusRetries = 0
                // Take the focus back now rather than next frame, for the same reason a click does:
                // whatever is typed while the model rebuilds must land in the block being edited.
                if (!root.focusBlock(root.focusRow)) {
                    Qt.callLater(root.focusBlock, root.focusRow)
                }
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

    // Clicking a rendered block enters edit mode at the caret the click landed on. Focus is taken in the
    // same event, not on the next frame: a keystroke typed the instant the block is clicked has to land
    // in it, and a frame's delay loses the character. The deferred attempt stays for the case where the
    // delegate for that row is not instantiated yet.
    function beginEdit(row, caret) {
        root.editingRow = row
        root.focusRow = row
        root.focusCursor = Math.max(0, caret)
        root.wantsFocus = true
        root.focusRetries = 0
        if (!root.focusBlock(row)) {
            Qt.callLater(root.focusBlock, row)
        }
    }

    function focusBlock(row) {
        if (row < 0 || row >= list.count) {
            return false
        }
        // Moving the caret into a block — with the arrow keys, or after a rebuild — is editing it, so
        // the row shows its editor rather than its rendered form.
        root.editingRow = row
        root.focusRow = row
        root.wantsFocus = true
        list.currentIndex = row
        list.positionViewAtIndex(row, ListView.Contain)

        const item = list.itemAtIndex(row)
        if (item && item.blockEditor && !item.isHeading) {
            item.blockEditor.forceActiveFocus()
            if (root.focusTerm !== "") {
                const at = item.blockEditor.text.toLowerCase().indexOf(root.focusTerm.toLowerCase())
                if (at >= 0) {
                    root.focusCursor = at
                }
                root.focusTerm = ""
            }
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
        if (marker === "TODO") return theme.warning
        if (marker === "DOING") return theme.accent
        if (marker === "DONE") return theme.success
        return theme.markerIdle
    }

    function formatDay(iso) {
        if (!iso) return ""
        return Qt.formatDate(new Date(iso + "T12:00:00"), "MMM d")
    }

    ListModel {
        id: searchModel
    }

    // ---- the quick switcher ------------------------------------------------------------------
    // Ctrl+K over anything: page names first, then blocks whose text matches.
    Shortcut {
        sequences: ["Ctrl+K"]
        onActivated: root.focusSearch()
    }

    Shortcut {
        sequences: ["Ctrl+B"]
        onActivated: root.sidebarOpen = !root.sidebarOpen
    }

    // Typing in the bar is what opens the results; Ctrl+K only puts the caret there.
    function focusSearch() {
        root.editingRow = -1
        searchField.forceActiveFocus()
        searchField.selectAll()
        if (searchField.text !== "") {
            searchPopup.open()
        }
    }

    function searchMove(step) {
        if (searchModel.count === 0) {
            return
        }
        root.searchSelected = (root.searchSelected + step + searchModel.count) % searchModel.count
        searchList.positionViewAtIndex(root.searchSelected, ListView.Contain)
    }

    function searchActivate(index) {
        if (index < 0 || index >= searchModel.count) {
            return
        }
        const hit = searchModel.get(index)
        searchPopup.close()
        // Give the caret back to the content: leaving it in the field meant the next keystroke landed in
        // the search box instead of the page that had just opened.
        root.pendingTerm = searchField.text.trim()
        searchField.text = ""
        searchField.focus = false
        if (hit.kind === "page") {
            root.openLink("page:" + hit.page)
        } else {
            root.rebuilding = true
            journal.open_block(hit.blockId, hit.page, hit.date)
        }
    }

    // Where a hit lives: a page's name, a day's date, or "today".
    function searchWhere(hit) {
        if (!/^\d{4}-\d{2}-\d{2}$/.test(hit.where)) {
            return hit.where
        }
        const today = Qt.formatDate(new Date(), "yyyy-MM-dd")
        return hit.where === today ? qsTr("today") : formatDay(hit.where)
    }

    Timer {
        id: searchDebounce
        interval: 120
        onTriggered: {
            journal.search(searchField.text)
            if (searchField.text.trim() !== "") {
                searchPopup.open()
            } else {
                searchPopup.close()
            }
        }
    }

    // "from Nov 1" goes to the block itself: its journal day, or the page it lives on, with the caret
    // in it — the same move a search hit makes.
    function openSource(blockId, source) {
        searchField.focus = false
        const isDate = /^\d{4}-\d{2}-\d{2}$/.test(source)
        root.rebuilding = true
        journal.open_block(blockId, isDate ? "" : source, isDate ? source : "")
    }

    // One place decides what following a link means: a page link opens that page, anything else leaves
    // the app. Page links and tags share the `page:` scheme, so they behave the same everywhere.
    function openLink(href) {
        if (href.indexOf("page:") === 0) {
            root.openPage(href.substring("page:".length))
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

        background: Rectangle {
            color: theme.page
            border.color: theme.border
            radius: 8
        }

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

                QuietButton {
                    text: "\u2039"
                    onClicked: datePopup.monthAnchor = new Date(datePopup.monthAnchor.getFullYear(),
                                                              datePopup.monthAnchor.getMonth() - 1, 1)
                }

                Label {
                    text: Qt.formatDate(datePopup.monthAnchor, "MMMM yyyy")
                    width: 150
                    horizontalAlignment: Text.AlignHCenter
                    anchors.verticalCenter: parent.verticalCenter
                    color: theme.text
                }

                QuietButton {
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
                onClicked: (pickedDay) => datePopup.picked = pickedDay
            }

            Label {
                text: qsTr("Picked: %1").arg(Qt.formatDate(datePopup.picked, "ddd, MMM d"))
                color: theme.textMuted
                font.pixelSize: 12
            }

            Row {
                spacing: 8
                anchors.right: parent.right

                QuietButton {
                    text: qsTr("Clear")
                    onClicked: {
                        root.rebuilding = true
                        journal.set_scheduled(datePopup.row, "")
                        datePopup.close()
                    }
                }

                QuietButton {
                    text: qsTr("Today")
                    onClicked: {
                        datePopup.picked = new Date()
                        datePopup.monthAnchor = new Date(datePopup.picked.getFullYear(),
                                                         datePopup.picked.getMonth(), 1)
                    }
                }

                QuietButton {
                    text: qsTr("OK")
                    onClicked: datePopup.accept()
                }
            }
        }

    }

    // ---- the shell: a bar across the top, the sidebar under it -------------------------------
    Rectangle {
        id: topBar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 48
        color: theme.panel

        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: theme.rule
        }

        Label {
            id: brand
            anchors.left: parent.left
            anchors.leftMargin: 20
            anchors.verticalCenter: parent.verticalCenter
            text: qsTr("LogNotes")
            color: theme.text
            font.pixelSize: 14
            font.bold: true
        }

        Row {
            id: barButtons
            anchors.right: parent.right
            anchors.rightMargin: 14
            anchors.verticalCenter: parent.verticalCenter
            spacing: 6

            QuietButton {
                objectName: "journalButton"
                text: qsTr("Journal")
                onClicked: root.goJournal()
            }

            QuietButton {
                objectName: "sidebarButton"
                text: root.sidebarOpen ? qsTr("\u2039") : qsTr("\u203a")
                onClicked: root.sidebarOpen = !root.sidebarOpen
            }

            QuietButton {
                objectName: "themeButton"
                text: root.darkTheme ? qsTr("\u2600") : qsTr("\u263e")
                onClicked: journal.apply_theme(root.darkTheme ? "light" : "dark")
            }
        }

        // The search field: typing in it opens the results just below.
        Rectangle {
            id: searchFieldBox
            anchors.left: brand.right
            anchors.leftMargin: 18
            anchors.right: barButtons.left
            anchors.rightMargin: 14
            anchors.verticalCenter: parent.verticalCenter
            height: 28
            radius: 14
            color: theme.page
            border.color: searchField.activeFocus ? theme.accent : theme.border

            TextInput {
                id: searchField
                objectName: "searchField"
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                verticalAlignment: TextInput.AlignVCenter
                color: theme.text
                font.pixelSize: 13
                selectByMouse: true
                onTextChanged: searchDebounce.restart()
                onActiveFocusChanged: {
                    if (!activeFocus) {
                        searchPopup.close()
                    }
                }

                Keys.onPressed: (event) => {
                    if (event.key === Qt.Key_Down) {
                        root.searchMove(1)
                        event.accepted = true
                    } else if (event.key === Qt.Key_Up) {
                        root.searchMove(-1)
                        event.accepted = true
                    } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                        if (searchPopup.visible) {
                            root.searchActivate(root.searchSelected)
                            event.accepted = true
                        }
                    } else if (event.key === Qt.Key_Escape) {
                        searchPopup.close()
                        searchField.text = ""
                        event.accepted = true
                    }
                }
            }

            // A plain TextInput has no placeholder, so the hint is its own label underneath it.
            Label {
                anchors.left: parent.left
                anchors.leftMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                visible: searchField.text === ""
                text: qsTr("Search pages and notes…")
                color: theme.textGhost
                font.pixelSize: 13
            }
        }
    }

    // The sidebar: favourites, then what was opened most recently.
    Rectangle {
        id: sidebar
        objectName: "sidebar"
        visible: root.sidebarOpen
        anchors.top: topBar.bottom
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        width: 184
        color: theme.panel

        Rectangle {
            anchors.right: parent.right
            width: 1
            height: parent.height
            color: theme.rule
        }

        Flickable {
            anchors.fill: parent
            anchors.margins: 10
            contentHeight: sidebarColumn.height
            clip: true

            Column {
                id: sidebarColumn
                width: parent.width
                spacing: 2

                Label {
                    visible: root.sidebarFavourites.length > 0
                    text: qsTr("Favourites")
                    color: theme.textGhost
                    font.pixelSize: 10
                    font.capitalization: Font.AllUppercase
                    font.letterSpacing: 0.6
                    leftPadding: 6
                    topPadding: 2
                    bottomPadding: 2
                }

                Repeater {
                    model: root.sidebarFavourites

                    delegate: Item {
                        objectName: "sidebarRow"
                        width: sidebarColumn.width
                        height: 26
                        property bool hovered: false

                        Rectangle {
                            anchors.fill: parent
                            radius: 5
                            color: parent.hovered ? theme.hover : "transparent"
                        }

                        Label {
                            anchors.left: parent.left
                            anchors.leftMargin: 6
                            anchors.verticalCenter: parent.verticalCenter
                            text: "\u2605"
                            color: theme.warning
                            font.pixelSize: 10
                        }

                        Label {
                            anchors.left: parent.left
                            anchors.leftMargin: 20
                            anchors.right: parent.right
                            anchors.rightMargin: 6
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData
                            color: theme.text
                            font.pixelSize: 12
                            elide: Text.ElideRight
                        }

                        MouseArea {
                            anchors.fill: parent
                            hoverEnabled: true
                            onEntered: parent.hovered = true
                            onExited: parent.hovered = false
                            onClicked: root.openPage(modelData)
                        }
                    }
                }

                Label {
                    visible: root.sidebarRecent.length > 0
                    text: qsTr("Recent")
                    color: theme.textGhost
                    font.pixelSize: 10
                    font.capitalization: Font.AllUppercase
                    font.letterSpacing: 0.6
                    leftPadding: 6
                    topPadding: 12
                    bottomPadding: 2
                }

                Repeater {
                    model: root.sidebarRecent

                    delegate: Item {
                        objectName: "sidebarRow"
                        width: sidebarColumn.width
                        height: 26
                        property bool hovered: false

                        Rectangle {
                            anchors.fill: parent
                            radius: 5
                            color: parent.hovered ? theme.hover : "transparent"
                        }

                        Label {
                            anchors.left: parent.left
                            anchors.leftMargin: 6
                            anchors.right: parent.right
                            anchors.rightMargin: 6
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData
                            color: theme.text
                            font.pixelSize: 12
                            elide: Text.ElideRight
                        }

                        MouseArea {
                            anchors.fill: parent
                            hoverEnabled: true
                            onEntered: parent.hovered = true
                            onExited: parent.hovered = false
                            onClicked: root.openPage(modelData)
                        }
                    }
                }
            }
        }
    }

    Popup {
        id: searchPopup
        objectName: "searchPopup"
        parent: Overlay.overlay
        // Hangs under the field it answers. Not modal and never focused: the field keeps the caret,
        // and Esc, Enter and the arrows are handled there.
        x: searchFieldBox.mapToItem(Overlay.overlay, 0, 0).x
        y: topBar.height + 6
        width: searchFieldBox.width
        padding: 0
        modal: false
        focus: false
        closePolicy: Popup.NoAutoClose

        background: Rectangle {
            color: theme.page
            border.color: theme.border
            radius: 10
        }

        contentItem: Column {
            spacing: 0

            ListView {
                id: searchList
                objectName: "searchList"
                width: parent.width
                height: {
                    // Each group's first row carries a small heading above it.
                    let total = 0
                    for (let i = 0; i < searchModel.count; i++) {
                        total += 30 + ((i === 0 || i === root.searchFirstBlock) ? 20 : 0)
                    }
                    return Math.min(320, total)
                }
                model: searchModel
                clip: true
                interactive: false
                boundsBehavior: Flickable.StopAtBounds

                delegate: Item {
                    width: searchList.width
                    height: 30 + (labelsGroup ? 20 : 0)
                    property bool labelsGroup: index === 0 || index === root.searchFirstBlock

                    Label {
                        id: groupLabel
                        visible: parent.labelsGroup
                        anchors.top: parent.top
                        anchors.left: parent.left
                        anchors.leftMargin: 14
                        height: 20
                        verticalAlignment: Text.AlignBottom
                        text: model.kind === "page" ? qsTr("Pages") : qsTr("Blocks")
                        color: theme.textFaint
                        font.pixelSize: 10
                        font.capitalization: Font.AllUppercase
                        font.letterSpacing: 0.4
                    }

                    Rectangle {
                        id: rowBody
                        anchors.top: parent.labelsGroup ? groupLabel.bottom : parent.top
                        width: parent.width
                        height: 30
                        color: index === root.searchSelected ? theme.hover : "transparent"

                        Label {
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.left: parent.left
                            anchors.leftMargin: 14
                            anchors.right: hitWhere.left
                            anchors.rightMargin: 10
                            // A block hit is already rendered from the scan; a page name is a plain name.
                            text: model.title
                            textFormat: model.kind === "block" ? Text.RichText : Text.PlainText
                            color: theme.text
                            font.pixelSize: 14
                            elide: Text.ElideRight
                        }

                        Label {
                            id: hitWhere
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.right: parent.right
                            anchors.rightMargin: 14
                            text: model.kind === "page" ? qsTr("page") : root.searchWhere(model)
                            color: theme.textFaint
                            font.pixelSize: 12
                        }

                        MouseArea {
                            anchors.fill: parent
                            hoverEnabled: true
                            onEntered: root.searchSelected = index
                            onClicked: root.searchActivate(index)
                        }
                    }
                }
            }

            Label {
                visible: root.searchMessage !== ""
                width: parent.width - 28
                x: 14
                topPadding: 10
                bottomPadding: 12
                text: root.searchMessage
                color: theme.dangerText
                font.pixelSize: 13
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
            }

            Label {
                visible: root.searchMessage === "" && searchModel.count > 0
                width: parent.width - 28
                x: 14
                topPadding: 8
                bottomPadding: 10
                text: qsTr("↑↓ to move · Enter to open · Esc to close")
                color: theme.textGhost
                font.pixelSize: 11
            }
        }
    }

    ListView {
        id: list
        objectName: "column"
        anchors.top: topBar.bottom
        anchors.bottom: parent.bottom
        anchors.left: root.sidebarOpen ? sidebar.right : parent.left
        anchors.right: parent.right
        anchors.margins: 28
        model: journal
        spacing: 3
        clip: true
        // No off-screen delegate cache: every save rebuilds the model, and a cached delegate from before
        // the rebuild lingers in the tree — invisible on screen, visible to anything that walks it.
        cacheBuffer: 0
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
            objectName: "blockRow"
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
            property bool isBlockRow: isBlock
            property bool editing: root.editingRow === index
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
                color: kind === "page-header" ? theme.text : theme.textMuted
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

            // A page is starred from its own header: the place you are looking at when you decide. An
            // item of its own, so the target is a comfortable square rather than the glyph.
            Item {
                id: favouriteStar
                objectName: "favouriteStar"
                visible: kind === "page-header"
                width: 32
                height: 32
                anchors.right: parent.right
                anchors.bottom: sectionHeading.bottom
                anchors.bottomMargin: -6

                Label {
                    anchors.centerIn: parent
                    text: journal.page_favourite ? "\u2605" : "\u2606"
                    color: journal.page_favourite ? theme.warning : theme.textGhost
                    font.pixelSize: 18
                }

                MouseArea {
                    anchors.fill: parent
                    onClicked: journal.toggle_favourite(model.text, !journal.page_favourite)
                }
            }

            // Anchored layout rather than a Row: an editor whose width is computed from its own
            // x inside a Row is a binding loop, and Qt resolves it by collapsing the width to 0.
            Item {
                id: blockRow
                visible: !rowItem.isHeading && !rowItem.isCollapsedMention
                width: parent.width
                height: Math.max(editor.visible ? editor.implicitHeight
                                                : (rendered.visible ? rendered.implicitHeight : 0), 24)

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
                        color: theme.page
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
                    color: model.overdue ? theme.danger : theme.textFaint
                    font.pixelSize: 12
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                }

                TextEdit {
                    id: editor
                    objectName: "blockEditor"
                    visible: rowItem.isBlock && (rowItem.editing || model.text === "")
                    anchors.left: markerLabel.visible ? markerLabel.right : markerBox.right
                    anchors.leftMargin: 8
                    anchors.right: scheduledChip.visible ? scheduledChip.left : parent.right
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    text: model.text
                    color: model.marker === "DONE" ? theme.textFaint : theme.text
                    font.pixelSize: 16
                    font.strikeout: model.marker === "DONE"
                    wrapMode: TextEdit.Wrap
                    selectByMouse: true
                    activeFocusOnPress: true

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
                            root.editingRow = index
                            if (root.wantsFocus) {
                                cursorPosition = Math.min(root.focusCursor, length)
                            }
                        } else if (!root.rebuilding && root.editingRow === index) {
                            // The block goes back to its rendered form once the caret has left it.
                            root.editingRow = -1
                        }
                        if (!activeFocus && debounce.running && !root.rebuilding) {
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
                        // An input method composes a character before it is text — a dead key on the
                        // Spanish layout is one — and saving now would rebuild the model, destroy the
                        // editor holding the composition, and take the character with it. Wait for the
                        // composition to finish; that is when the text is really typed.
                        if (!editor.inputMethodComposing) {
                            debounce.restart()
                        }
                    }

                    onInputMethodComposingChanged: {
                        if (!inputMethodComposing) {
                            debounce.restart()
                        }
                    }

                    Keys.onPressed: (event) => {
                        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                            debounce.stop()
                            if ((event.modifiers & Qt.ShiftModifier)
                                    || journal.in_fence(editor.text, cursorPosition)) {
                                // A line break inside the block: a fence or a diagram needs one, while
                                // plain Enter must keep meaning "new block". Inside an open fence, Enter
                                // continues it — typing ``` should be all it takes.
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
                        } else if ((event.key === Qt.Key_B || event.key === Qt.Key_K)
                                   && (event.modifiers & Qt.ControlModifier)) {
                            // The window shortcuts for Ctrl+B and Ctrl+K do the work — a shortcut runs
                            // first but does not consume the key, so without this TextEdit would also
                            // act on it (bold for Ctrl+B, delete-to-end-of-line for Ctrl+K) and a
                            // delegated copy of the action would fire a second time. Consume only.
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

                // The same block, read rather than edited: marks gone, links still links. Clicking it
                // swaps in the editor and puts the caret where the click landed.
                TextEdit {
                    id: rendered
                    objectName: "blockRendered"
                    visible: rowItem.isBlock && !rowItem.editing && model.text !== ""
                    textFormat: TextEdit.RichText
                    text: model.html
                    readOnly: true
                    selectByMouse: false
                    anchors.left: markerLabel.visible ? markerLabel.right : markerBox.right
                    anchors.leftMargin: 8
                    anchors.right: scheduledChip.visible ? scheduledChip.left : parent.right
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    color: model.marker === "DONE" ? theme.textFaint : theme.text
                    font.pixelSize: 16
                    font.strikeout: model.marker === "DONE"
                    wrapMode: TextEdit.Wrap

                    // The pointer turns into a hand over a link and stays an I-beam elsewhere.
                    onLinkHovered: (link) => rendered.cursorShape =
                                    link === "" ? Qt.IBeamCursor : Qt.PointingHandCursor

                    // A click on a link follows it; a click anywhere else starts editing there. The tap
                    // handler takes the press, so the read-only view never gets to emit `linkActivated`
                    // and the link is resolved here, with the same hit test the view uses.
                    TapHandler {
                        onSingleTapped: (event) => {
                            const link = rendered.linkAt(event.position.x, event.position.y)
                            if (link !== "") {
                                root.openLink(link)
                                return
                            }
                            const shown = rendered.positionAt(event.position.x, event.position.y)
                            root.beginEdit(index, journal.raw_position(model.text, shown))
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
                    color: model.overdue ? theme.danger : theme.text
                    font.pixelSize: 16
                    elide: Text.ElideRight
                }

                Label {
                    id: sourceLabel
                    objectName: "sourceLabel"
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
                    color: model.overdue ? theme.danger : theme.textFaint
                    font.pixelSize: 12

                }

                // Only the "from …" label navigates, and the target sits over the label rather than
                // inside it: a MouseArea nested in a Label is not reliably hit.
                MouseArea {
                    anchors.fill: sourceLabel
                    anchors.margins: -4
                    enabled: rowItem.isReference
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.openSource(model.blockId, model.source)
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
                    color: theme.danger
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
        color: theme.page
        visible: !journal.ready && journal.error !== ""

        Column {
            anchors.centerIn: parent
            spacing: 14
            width: Math.min(parent.width - 80, 620)

            Label {
                text: qsTr("LogNotes could not open its database.")
                color: theme.text
                font.pixelSize: 18
                font.bold: true
            }

            Label {
                width: parent.width
                text: journal.error
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                color: theme.dangerText
            }

            Label {
                width: parent.width
                text: qsTr("Your notes are still on disk; LogNotes just cannot open them right now.")
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                color: theme.textMuted
            }

            Row {
                spacing: 10

                QuietButton {
                    text: qsTr("Retry")
                    onClicked: journal.start()
                }

                QuietButton {
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
        color: theme.dangerText
    }
}
