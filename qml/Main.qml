pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import QtQuick.Window
import org.kde.kirigami as Kirigami

Kirigami.ApplicationWindow {
    id: root
    width: 1280
    height: 880
    minimumWidth: 760
    minimumHeight: 600
    title: root.game.path ? root.game.tabs[root.game.active].title + " — Chess" : "Chess"
    visible: true
    font.family: "Noto Sans"
    font.pixelSize: 13
    color: "#f6f4ed"
    palette.window: "#f6f4ed"
    palette.windowText: "#252f28"
    palette.base: "#fffefa"
    palette.text: "#252f28"
    palette.button: "#fffefa"
    palette.buttonText: "#344c3c"
    palette.highlight: "#3e5b48"
    palette.highlightedText: "#fffef8"
    palette.mid: "#c6cbbd"
    Kirigami.Theme.inherit: false
    Kirigami.Theme.backgroundColor: "#f6f4ed"
    Kirigami.Theme.textColor: "#252f28"
    Kirigami.Theme.highlightColor: "#3e5b48"
    Kirigami.Theme.highlightedTextColor: "#fffef8"
    readonly property var game: chess.state
    readonly property var prefs: root.game.preferences
    property string savePurpose: "game"
    property bool closingDocument: false
    property int keyboardSquare: 12
    readonly property bool editingText: root.activeFocusItem !== null && root.activeFocusItem.cursorPosition !== undefined
    readonly property bool flipped: ((root.prefs.view.yaw % 360 + 360) % 360) > 90 && ((root.prefs.view.yaw % 360 + 360) % 360) < 270
    pageStack.globalToolBar.style: Kirigami.ApplicationHeaderStyle.None
    onClosing: close => { close.accepted = root.game.close_allowed; if (!root.game.close_allowed) command("quit"); }
    function command(name, values) { return chess.command(Object.assign({command: name}, values || {})); }
    function action(name, data) { return command("action", {name: name, data: data || {}}); }
    function preference(patch) { action("preferences", patch); }
    function view(patch) { preference({view: patch}); }
    function saveGame(asNew) {
        savePurpose = "game";
        if (!asNew && root.game.path) command("save", {path: root.game.path});
        else saveDialog.open();
    }
    function path(url) { return decodeURIComponent(url.toString().replace(/^file:\/\//, "")); }
    function playerName(white) {
        const key = white ? "White" : "Black";
        const name = root.game.headers[key];
        return name && name !== key ? name : root.game.computer[white ? 0 : 1] ? "Computer" : key + " player";
    }
    function closePopups() { settings.close(); newGame.close(); networkDialog.close(); infoDialog.close(); positionDialog.close(); helpDialog.close(); }

    Connections {
        target: chess
        function onUiAction(action) {
            if (action === "settings") settings.open();
            else if (action === "new") newGame.open();
            else if (action === "network") networkDialog.open();
            else if (action === "close_dialogs") root.closePopups();
            else if (action === "appearance") { settings.selectedPage = 0; settings.open(); }
            else if (action === "computer") { settings.selectedPage = 1; settings.open(); }
            else if (action === "speech") { settings.selectedPage = 2; settings.open(); }
            else if (action === "materials") { settings.selectedPage = 3; settings.open(); }
        }
    }
    header: Rectangle {
        height: 76
        color: "#fffefa"
        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: "#e1e2d8" }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 26
            anchors.rightMargin: 26
            spacing: 20
            Rectangle {
                implicitWidth: 40; implicitHeight: 40; radius: 12; color: "#3e5b48"
                Controls.Label { anchors.centerIn: parent; text: "♔"; font.pixelSize: 29; color: "#fffef8" }
            }
            Controls.Label { text: "Chess"; font.family: "Noto Serif"; font.pixelSize: 25; color: "#252f28" }
            RowLayout {
                spacing: 2
                SoftButton { text: "Game"; quiet: true; onClicked: gameMenu.popup(); }
                SoftButton { text: "Moves"; quiet: true; onClicked: movesMenu.popup(); }
                SoftButton { text: "View"; quiet: true; onClicked: viewMenu.popup(); }
                SoftButton { text: "Share"; quiet: true; onClicked: shareMenu.popup(); }
            }
            Item { Layout.fillWidth: true }
            SoftButton { text: "New game"; icon.name: "list-add"; primary: true; enabled: !root.game.network_active; onClicked: newGame.open(); }
            SoftButton { icon.name: "settings-configure"; quiet: true; Accessible.name: "Preferences"; onClicked: settings.open(); }
        }
    }
    footer: Rectangle {
        height: 64
        color: "#fffefa"
        Rectangle { width: parent.width; height: 1; color: "#e1e2d8" }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 26
            anchors.rightMargin: 26
            spacing: 12
            Kirigami.Icon { source: "input-keyboard"; implicitWidth: 20; implicitHeight: 20; color: "#6e776d" }
            Controls.TextField {
                id: moveInput
                objectName: "moveInput"
                Layout.preferredWidth: 340
                Layout.maximumWidth: root.width * 0.4
                implicitHeight: 38
                placeholderText: "Enter a move — e2e4, Nf3, knight to f three"
                Accessible.name: "Move or spoken command"
                onAccepted: { root.action("voice", {text: text}); text = ""; }
            }
            SoftButton { text: "Play move"; icon.name: "go-next"; onClicked: { root.action("voice", {text: moveInput.text}); moveInput.text = ""; } }
            Item { Layout.fillWidth: true }
            Controls.Label { visible: root.width > 1050; text: root.game.recording ? "● Recording" : root.game.connected ? "Network game" : "Local game"; color: root.game.recording ? "#a85843" : "#6e776d"; font.pixelSize: 12 }
            SoftButton { quiet: true; icon.name: root.game.listening ? "microphone-sensitivity-high" : "audio-input-microphone"; Accessible.name: root.game.listening ? "Stop listening" : "Listen for spoken moves"; onClicked: root.action("listen", {enabled: !root.game.listening}); }
            SoftButton { quiet: true; icon.name: "help-contextual"; Accessible.name: "Help and keyboard shortcuts"; onClicked: helpDialog.open(); }
        }
    }
    Controls.Menu {
        id: gameMenu
        Controls.MenuItem { text: "New game…"; icon.name: "document-new"; enabled: !root.game.network_active; onTriggered: newGame.open(); }
        Controls.MenuItem { text: "Open game…"; icon.name: "document-open"; enabled: !root.game.network_active; onTriggered: openDialog.open(); }
        Controls.Menu {
            id: recentMenu
            title: "Recent games"
            Instantiator {
                model: root.prefs.recent
                delegate: Controls.MenuItem { required property string modelData; text: modelData.split("/").pop(); onTriggered: root.command("open", {path: modelData}); }
                onObjectAdded: (index, object) => recentMenu.insertItem(index, object)
                onObjectRemoved: (index, object) => recentMenu.removeItem(object)
            }
        }
        Controls.MenuItem { text: "Duplicate game"; enabled: !root.game.network_active; onTriggered: root.action("duplicate"); }
        Controls.MenuSeparator {}
        Controls.MenuItem { text: "Save"; icon.name: "document-save"; onTriggered: root.saveGame(false); }
        Controls.MenuItem { text: "Save as…"; icon.name: "document-save-as"; onTriggered: root.saveGame(true); }
        Controls.MenuItem { text: "Game information…"; icon.name: "document-properties"; onTriggered: infoDialog.open(); }
        Controls.MenuItem { text: "Copy position"; onTriggered: chess.copy(root.game.fen); }
        Controls.MenuItem { text: "Copy game (PGN)"; onTriggered: root.action("copy_pgn"); }
        Controls.MenuItem { text: "Set up a position…"; enabled: !root.game.network_active; onTriggered: { fenInput.text = root.game.fen; positionDialog.open(); } }
        Controls.MenuSeparator {}
        Controls.MenuItem { text: "Quit"; icon.name: "application-exit"; onTriggered: root.command("quit"); }
    }
    Controls.Menu {
        id: movesMenu
        Controls.MenuItem { text: "Take back move"; icon.name: "edit-undo"; onTriggered: root.command("undo"); }
        Controls.MenuItem { text: "Redo move"; icon.name: "edit-redo"; enabled: !root.game.network_active && root.game.ply < root.game.total; onTriggered: root.command("seek", {ply: root.game.ply + 1}); }
        Controls.MenuItem { text: "Suggest a move"; icon.name: "help-hint"; onTriggered: root.command("hint"); }
        Controls.MenuItem { text: root.game.paused ? "Resume computer play" : "Pause computer play"; enabled: !root.game.network_active; onTriggered: root.command("pause", {paused: !root.game.paused}); }
        Controls.MenuItem { text: "Show last move"; checkable: true; checked: root.game.show_last; onTriggered: root.action("show_last", {enabled: !root.game.show_last}); }
        Controls.MenuSeparator {}
        Controls.MenuItem { text: "Offer a draw"; onTriggered: root.command("ask", {request: "draw"}); }
        Controls.MenuItem { text: "Resign…"; onTriggered: resignDialog.open(); }
    }
    Controls.Menu {
        id: viewMenu
        Controls.MenuItem { text: "Rotate board"; icon.name: "object-flip-horizontal"; onTriggered: root.action("flip"); }
        Controls.MenuItem { text: "2D accessible board"; checkable: true; checked: root.prefs.view.flat; onTriggered: root.view({flat: !root.prefs.view.flat}); }
        Controls.MenuItem { text: "Board coordinates"; checkable: true; checked: root.prefs.view.coordinates; onTriggered: root.view({coordinates: !root.prefs.view.coordinates}); }
        Controls.MenuItem { text: "Move animations"; checkable: true; checked: root.prefs.view.animations; onTriggered: root.view({animations: !root.prefs.view.animations}); }
        Controls.MenuItem { text: "Show game panel"; checkable: true; checked: root.prefs.show_log; onTriggered: root.preference({show_log: !root.prefs.show_log}); }
        Controls.MenuItem { text: "Reset camera"; onTriggered: root.view({yaw: 0, elevation: 55, distance: 13.7}); }
        Controls.MenuSeparator {}
        Controls.MenuItem { text: "Full screen"; icon.name: "view-fullscreen"; onTriggered: root.visibility = root.visibility === Window.FullScreen ? Window.Windowed : Window.FullScreen; }
        Controls.MenuItem { text: "Always on top"; checkable: true; checked: (root.flags & Qt.WindowStaysOnTopHint) !== 0; onTriggered: root.flags = checked ? root.flags | Qt.WindowStaysOnTopHint : root.flags & ~Qt.WindowStaysOnTopHint; }
        Controls.MenuItem { text: "Appearance…"; icon.name: "draw-brush"; onTriggered: { settings.selectedPage = 0; settings.open(); } }
    }
    Controls.Menu {
        id: shareMenu
        Controls.MenuItem { text: "Network game…"; icon.name: "network-connect"; onTriggered: networkDialog.open(); }
        Controls.MenuItem { text: "Save screenshot…"; icon.name: "camera-photo"; onTriggered: { savePurpose = "screenshot"; saveDialog.open(); } }
        Controls.MenuItem { text: root.game.recording ? "Stop recording" : "Record game…"; icon.name: "media-record"; onTriggered: { if (root.game.recording) root.action("stop_record"); else { savePurpose = "record"; saveDialog.open(); } } }
    }
    pageStack.initialPage: Kirigami.Page {
        id: playPage
        padding: 24
        title: ""
        background: Rectangle { color: "#f6f4ed" }
        ColumnLayout {
            anchors.fill: parent
            spacing: 14
            RowLayout {
                visible: root.game.tabs.length > 1
                Layout.fillWidth: true
                spacing: 8
                Controls.ScrollView {
                    Layout.fillWidth: true
                    implicitHeight: 37
                    Controls.ScrollBar.vertical.policy: Controls.ScrollBar.AlwaysOff
                    Row {
                        spacing: 8
                        Repeater {
                            model: root.game.tabs
                            delegate: Rectangle {
                                required property var modelData
                                required property int index
                                height: 36; width: tabLabel.implicitWidth + 57
                                radius: 7; color: index === root.game.active ? "#e4ebdd" : "#fffefa"
                                border.color: index === root.game.active ? "#c5d0ba" : "#e1e2d8"
                                Row {
                                    anchors.centerIn: parent; spacing: 6
                                    Controls.ToolButton { id: tabLabel; text: modelData.title + (modelData.dirty ? " ·" : ""); enabled: !root.game.network_active; onClicked: root.action("switch", {index: index}); }
                                    Controls.ToolButton { text: "×"; implicitWidth: 24; enabled: !root.game.network_active; Accessible.name: "Close " + modelData.title; onClicked: root.action("close", {index: index}); }
                                }
                            }
                        }
                    }
                }
                SoftButton { text: "+"; quiet: true; implicitWidth: 34; enabled: !root.game.network_active; Accessible.name: "New game"; onClicked: newGame.open(); }
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 24
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: 18
                    RowLayout {
                        Layout.fillWidth: true
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 5
                            Controls.Label { text: root.game.variant_name.toUpperCase() + " CHESS"; color: "#788371"; font.pixelSize: 10; font.letterSpacing: 1.7; font.weight: Font.Medium }
                            Kirigami.Heading { text: root.game.headers.Event || "Casual game"; font.family: "Noto Serif"; font.pixelSize: root.width > 1000 ? 29 : 24; color: "#252f28"; elide: Text.ElideRight; Layout.fillWidth: true }
                        }
                        Rectangle {
                            color: "#e9e9df"; radius: 9; implicitWidth: modeRow.implicitWidth + 8; implicitHeight: 38
                            Row {
                                id: modeRow; anchors.centerIn: parent; spacing: 1
                                Repeater {
                                    model: ["3D", "2D"]
                                    delegate: SoftButton {
                                        required property string modelData
                                        required property int index
                                        text: modelData; implicitWidth: 45; implicitHeight: 30; leftPadding: 8; rightPadding: 8
                                        checked: root.prefs.view.flat === (index === 1)
                                        Accessible.name: modelData + " board"
                                        onClicked: root.view({flat: index === 1});
                                    }
                                }
                            }
                        }
                        SoftButton { icon.name: "object-flip-horizontal"; quiet: true; Accessible.name: "Rotate board"; onClicked: root.action("flip"); }
                    }
                    Rectangle {
                        id: stage
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        color: "#efebe1"
                        radius: 18
                        border.color: "#dedccf"
                        clip: true
                        focus: true
                        Keys.onPressed: event => {
                            if (event.key === Qt.Key_Escape) root.action("clear_selection");
                            else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) root.action("square", {square: root.game.squares[root.keyboardSquare].square});
                            else if (event.key === Qt.Key_Left) root.keyboardSquare = Math.max(0, root.keyboardSquare + (root.flipped ? 1 : -1));
                            else if (event.key === Qt.Key_Right) root.keyboardSquare = Math.min(63, root.keyboardSquare + (root.flipped ? -1 : 1));
                            else if (event.key === Qt.Key_Up) root.keyboardSquare = Math.max(0, Math.min(63, root.keyboardSquare + (root.flipped ? -8 : 8)));
                            else if (event.key === Qt.Key_Down) root.keyboardSquare = Math.max(0, Math.min(63, root.keyboardSquare + (root.flipped ? 8 : -8)));
                            else return;
                            event.accepted = true;
                        }
                        Image {
                            id: boardImage
                            anchors.fill: parent
                            anchors.margins: 1
                            visible: !root.prefs.view.flat
                            source: visible ? "image://board/" + root.game.board_revision : ""
                            sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
                            cache: false
                            fillMode: Image.Stretch
                            Repeater {
                                model: root.prefs.view.flat ? [] : root.game.coordinates
                                delegate: Controls.Label {
                                    required property var modelData
                                    x: modelData.x * boardImage.width - width / 2
                                    y: modelData.y * boardImage.height - height / 2
                                    text: modelData.text
                                    font.pixelSize: 11
                                    color: "#e7dfc6"
                                    opacity: root.prefs.view.label_intensity
                                }
                            }
                            MouseArea {
                                anchors.fill: parent
                                acceptedButtons: Qt.LeftButton | Qt.RightButton
                                property point previous: Qt.point(0,0)
                                property point origin: Qt.point(0,0)
                                onPressed: mouse => {
                                    stage.forceActiveFocus(); previous = Qt.point(mouse.x, mouse.y); origin = previous;
                                    if (mouse.button === Qt.LeftButton) root.action("click", {x: mouse.x / width * boardImage.sourceSize.width, y: mouse.y / height * boardImage.sourceSize.height});
                                }
                                onPositionChanged: mouse => {
                                    if (pressedButtons & Qt.RightButton) root.action("orbit", {dx: mouse.x - previous.x, dy: mouse.y - previous.y});
                                    previous = Qt.point(mouse.x, mouse.y);
                                }
                                onReleased: mouse => {
                                    if (mouse.button === Qt.LeftButton && Math.abs(mouse.x-origin.x)+Math.abs(mouse.y-origin.y) > 5)
                                        root.action("click", {x: mouse.x / width * boardImage.sourceSize.width, y: mouse.y / height * boardImage.sourceSize.height});
                                }
                                onWheel: wheel => { root.action("zoom", {delta: wheel.angleDelta.y}); wheel.accepted = true; }
                            }
                        }
                        Grid {
                            visible: root.prefs.view.flat
                            anchors.centerIn: parent
                            columns: 8
                            readonly property real cell: Math.floor((Math.min(stage.width, stage.height) - 40) / 8)
                            Repeater {
                                model: 64
                                delegate: Controls.Button {
                                    required property int index
                                    readonly property int sqIndex: root.flipped ? (Math.floor(index/8))*8 + (7-index%8) : (7-Math.floor(index/8))*8 + index%8
                                    readonly property var piece: root.game.squares[sqIndex]
                                    readonly property bool target: root.game.targets.indexOf(piece.square) >= 0
                                    width: parent.cell; height: parent.cell
                                    padding: 0
                                    Accessible.name: piece.label
                                    Accessible.description: target ? "Legal destination" : ""
                                    onClicked: { root.keyboardSquare = sqIndex; root.action("square", {square: piece.square}); }
                                    background: Rectangle {
                                        color: root.game.selected === piece.square ? "#b4c491" : (piece.file+piece.rank)%2 === 0 ? "#90a184" : "#e3e7d7"
                                        border.width: parent.visualFocus ? 3 : 0
                                        border.color: "#3e5b48"
                                    }
                                    contentItem: Item {
                                        Controls.Label { anchors.centerIn: parent; text: piece.glyph; font.pixelSize: parent.height * 0.67; color: piece.white ? "#fffef4" : "#29362d"; style: Text.Outline; styleColor: piece.white ? "#69775f" : "#29362d" }
                                        Controls.Label { anchors.left: parent.left; anchors.top: parent.top; anchors.margins: 5; visible: root.prefs.view.coordinates; text: piece.square; font.pixelSize: 9; color: "#3b4c36" }
                                        Rectangle { anchors.centerIn: parent; width: 12; height: 12; radius: 6; color: "#4c7044"; opacity: 0.7; visible: target && !piece.glyph }
                                    }
                                }
                            }
                        }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        Controls.Label { text: root.flipped ? "Black’s perspective" : "White’s perspective"; font.pixelSize: 11; color: "#788371" }
                        Item { Layout.fillWidth: true }
                        Controls.Label { visible: !root.prefs.view.flat && root.width > 1100; text: "Drag to move  ·  Right-drag to orbit  ·  Scroll to zoom"; font.pixelSize: 11; color: "#788371" }
                        SoftButton { quiet: true; text: "Appearance"; icon.name: "draw-brush"; implicitHeight: 28; font.pixelSize: 11; onClicked: settings.open(); }
                    }
                }
                Controls.ScrollView {
                    id: gamePanel
                    visible: root.prefs.show_log
                    Layout.preferredWidth: root.width > 1000 ? 304 : 258
                    Layout.minimumWidth: root.width > 1000 ? 304 : 258
                    Layout.maximumWidth: root.width > 1000 ? 304 : 258
                    Layout.fillHeight: true
                    clip: true
                    Controls.ScrollBar.horizontal.policy: Controls.ScrollBar.AlwaysOff
                    ColumnLayout {
                        width: gamePanel.availableWidth
                        height: Math.max(implicitHeight, gamePanel.availableHeight)
                        spacing: 12
                        RowLayout {
                            Layout.fillWidth: true
                            Kirigami.Heading { text: root.game.status; level: 3; font.family: "Noto Serif"; font.pixelSize: 22; color: "#252f28"; Layout.fillWidth: true }
                            Controls.BusyIndicator { running: root.game.thinking; visible: running; implicitWidth: 22; implicitHeight: 22 }
                        }
                        PlayerCard { Layout.fillWidth: true; playerName: root.playerName(false); detail: (root.game.computer[1] ? "Computer" : "Human") + " · Black"; whiteSide: false; toMove: !root.game.white_turn && root.game.result === "*" }
                        PlayerCard { Layout.fillWidth: true; playerName: root.playerName(true); detail: (root.game.computer[0] ? "Computer" : "Human") + " · White"; toMove: root.game.white_turn && root.game.result === "*" }
                        RowLayout {
                            visible: root.game.computer[0] || root.game.computer[1] || root.game.network_active
                            Layout.fillWidth: true
                            Controls.Label { text: root.game.network_active ? root.game.local_turn ? "Your turn" : "Opponent’s turn" : root.game.thinking ? "Considering the position…" : root.game.paused ? "Computer play paused" : "Computer ready"; font.pixelSize: 11; color: "#6e776d"; Layout.fillWidth: true }
                            SoftButton { visible: !root.game.network_active; text: root.game.paused ? "Resume" : "Pause"; quiet: true; implicitHeight: 28; onClicked: root.command("pause", {paused: !root.game.paused}); }
                        }
                        Rectangle {
                            visible: root.game.variant === "crazyhouse"
                            Layout.fillWidth: true
                            implicitHeight: pocketColumn.implicitHeight + 20
                            color: "#fffefa"; radius: 10; border.color: "#e1e2d8"
                            ColumnLayout {
                                id: pocketColumn; anchors.fill: parent; anchors.margins: 10; spacing: 4
                                Controls.Label { text: "PIECES IN HAND"; font.pixelSize: 10; color: "#788371"; font.letterSpacing: 1 }
                                Repeater {
                                    model: root.game.pockets
                                    delegate: RowLayout {
                                        required property var modelData
                                        Controls.Label { text: modelData.white ? "White" : "Black"; font.pixelSize: 11; color: "#6e776d"; Layout.preferredWidth: 36 }
                                        Repeater {
                                            model: modelData.pieces
                                            delegate: SoftButton {
                                                required property var modelData
                                                text: modelData.glyph + " ×" + modelData.count; font.pixelSize: 16; implicitHeight: 32; leftPadding: 5; rightPadding: 5
                                                enabled: root.game.local_turn && parent.modelData.white === root.game.white_turn
                                                Accessible.name: "Drop " + modelData.role
                                                onClicked: root.action("drop", {role: modelData.role});
                                            }
                                        }
                                        Controls.Label { visible: parent.modelData.pieces.length === 0; text: "No captured pieces"; font.pixelSize: 11; color: "#8e958a" }
                                    }
                                }
                            }
                        }
                        Rectangle {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            Layout.minimumHeight: 178
                            color: "#fffefa"; radius: 12; border.color: "#e1e2d8"
                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 15
                                spacing: 10
                                RowLayout {
                                    Controls.Label { text: "Move history"; font.pixelSize: 14; font.weight: Font.Medium; color: "#252f28"; Layout.fillWidth: true }
                                    Controls.Label { text: root.game.ply < root.game.total ? "Reviewing" : root.game.ply + " plies"; font.pixelSize: 10; color: "#788371" }
                                }
                                RowLayout {
                                    Layout.fillWidth: true
                                    Controls.Label { text: "#"; color: "#909789"; font.pixelSize: 10; Layout.preferredWidth: 28 }
                                    Controls.Label { text: "WHITE"; color: "#788371"; font.pixelSize: 10; font.letterSpacing: 1; Layout.fillWidth: true }
                                    Controls.Label { text: "BLACK"; color: "#788371"; font.pixelSize: 10; font.letterSpacing: 1; Layout.fillWidth: true }
                                }
                                ListView {
                                    id: historyList
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    clip: true
                                    model: root.game.history
                                    spacing: 3
                                    Controls.ScrollBar.vertical: Controls.ScrollBar { policy: Controls.ScrollBar.AsNeeded }
                                    delegate: RowLayout {
                                        required property var modelData
                                        required property int index
                                        width: historyList.width - 8
                                        height: 34
                                        spacing: 5
                                        Controls.Label { text: modelData.number + "."; color: "#858e80"; font.pixelSize: 12; Layout.preferredWidth: 28 }
                                        Repeater {
                                            model: [modelData.white, modelData.black]
                                            delegate: SoftButton {
                                                required property var modelData
                                                Layout.fillWidth: true
                                                Layout.preferredWidth: 90
                                                implicitHeight: 30
                                                text: modelData ? modelData.san + (modelData.comment ? " ·" : "") : ""
                                                checked: modelData && root.game.ply === modelData.ply
                                                quiet: !checked
                                                enabled: !!modelData && !root.game.network_active
                                                onClicked: root.command("seek", {ply: modelData.ply});
                                                Controls.ToolTip.text: modelData && modelData.comment ? modelData.comment : ""
                                            }
                                        }
                                    }
                                    Column {
                                        visible: root.game.history.length === 0
                                        anchors.centerIn: parent
                                        spacing: 8
                                        width: parent.width
                                        Controls.Label { text: "♙"; anchors.horizontalCenter: parent.horizontalCenter; font.pixelSize: 44; color: "#8c9b7c" }
                                        Controls.Label { text: "The opening is yours."; anchors.horizontalCenter: parent.horizontalCenter; font.family: "Noto Serif"; font.pixelSize: 16; color: "#4e6048" }
                                        Controls.Label { text: "Your moves will appear here."; anchors.horizontalCenter: parent.horizontalCenter; font.pixelSize: 11; color: "#8a9282" }
                                    }
                                }
                                Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: "#e9e9df" }
                                RowLayout {
                                    spacing: 2
                                    SoftButton { icon.name: "go-first"; quiet: true; implicitWidth: 32; leftPadding: 7; rightPadding: 7; enabled: !root.game.network_active && root.game.ply > 0; Accessible.name: "Initial position"; onClicked: root.command("seek", {ply: 0}); }
                                    SoftButton { icon.name: "go-previous"; quiet: true; implicitWidth: 32; leftPadding: 7; rightPadding: 7; enabled: root.game.ply > 0; Accessible.name: "Take back move"; onClicked: root.command("undo"); }
                                    SoftButton { icon.name: "go-next"; quiet: true; implicitWidth: 32; leftPadding: 7; rightPadding: 7; enabled: !root.game.network_active && root.game.ply < root.game.total; Accessible.name: "Next move"; onClicked: root.command("seek", {ply: root.game.ply+1}); }
                                    SoftButton { icon.name: "go-last"; quiet: true; implicitWidth: 32; leftPadding: 7; rightPadding: 7; enabled: !root.game.network_active && root.game.ply < root.game.total; Accessible.name: "Latest position"; onClicked: root.command("seek", {ply: root.game.total}); }
                                    Item { Layout.fillWidth: true }
                                    SoftButton { text: "Hint"; icon.name: "help-hint"; implicitHeight: 32; leftPadding: 10; rightPadding: 10; onClicked: root.command("hint"); }
                                }
                            }
                        }
                        Controls.Label { visible: !!root.game.hint; text: "Suggested move: " + root.game.hint; color: "#3e5b48"; font.pixelSize: 12 }
                        Controls.Label { visible: !!root.game.analysis; text: root.game.analysis ? "Depth " + root.game.analysis.depth + " · " + root.game.analysis.score.toFixed(2) + " · " + root.game.analysis.nodes.toLocaleString() + " positions" : ""; color: "#788371"; font.pixelSize: 10 }
                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 99
                            radius: 12; color: "#fffefa"; border.color: "#e1e2d8"
                            ColumnLayout {
                                anchors.fill: parent; anchors.margins: 13; spacing: 6
                                Controls.Label { text: "Position notes"; font.pixelSize: 12; font.weight: Font.Medium; color: "#5d6c55" }
                                Controls.TextArea {
                                    id: notes
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    placeholderText: "Add a thought about this position…"
                                    font.pixelSize: 11
                                    text: root.game.comment
                                    wrapMode: Text.WordWrap
                                    onTextChanged: if (activeFocus && text !== root.game.comment) root.action("comment", {text: text});
                                    Accessible.name: "Comment on current position"
                                }
                            }
                        }
                        Controls.ComboBox {
                            visible: root.game.variations.length > 0
                            Layout.fillWidth: true
                            model: root.game.variations.map((n, i) => "Variation " + (i+1) + " · " + n + " plies")
                            displayText: "Saved variations"
                            enabled: !root.game.network_active
                            onActivated: root.action("variation", {index: currentIndex});
                        }
                    }
                }
            }
            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: root.game.message.length > 0
                text: root.game.message
                showCloseButton: true
                type: Kirigami.MessageType.Information
                onVisibleChanged: if (!visible && root.game.message.length) root.action("clear_message");
            }
        }
    }
    Settings { id: settings; app: root }
    Kirigami.Dialog {
        id: newGame
        title: "A new game"
        preferredWidth: 540
        property string variant: "standard"
        ColumnLayout {
            spacing: 18
            Controls.Label { text: "Choose your game. Find your next move."; color: "#6e776d"; Layout.fillWidth: true }
            GridLayout {
                columns: 2
                columnSpacing: 12
                rowSpacing: 12
                Layout.fillWidth: true
                Repeater {
                    model: [
                {name:"Standard", value:"standard", detail:"The classic game. Protect your king."},
                        {name:"Crazyhouse", value:"crazyhouse", detail:"Bring captured pieces back into play."},
                        {name:"Suicide", value:"suicide", detail:"Lose all your pieces to win."},
                        {name:"Losers", value:"losers", detail:"Give up your army. Keep your king safe."}
                    ]
                    delegate: SoftButton {
                        required property var modelData
                        Layout.fillWidth: true
                        Layout.preferredWidth: 225
                        implicitHeight: 105
                        checked: newGame.variant === modelData.value
                        onClicked: newGame.variant = modelData.value
                        contentItem: ColumnLayout {
                            spacing: 8
                            Controls.Label { text: modelData.name; font.pixelSize: 15; font.weight: Font.Medium; color: "#344c3c" }
                            Controls.Label { text: modelData.detail; font.pixelSize: 11; color: "#6e776d"; wrapMode: Text.WordWrap; Layout.fillWidth: true }
                        }
                    }
                }
            }
            Kirigami.FormLayout {
                Layout.fillWidth: true
                Controls.ComboBox { id: newWhite; Kirigami.FormData.label: "White:"; model: ["Human", "Computer"]; Layout.fillWidth: true }
                Controls.ComboBox { id: newBlack; Kirigami.FormData.label: "Black:"; model: ["Human", "Computer"]; currentIndex: 1; Layout.fillWidth: true }
            }
            RowLayout {
                Item { Layout.fillWidth: true }
                SoftButton { text: "Cancel"; quiet: true; onClicked: newGame.close(); }
                SoftButton { text: "Start game"; primary: true; icon.name: "go-next"; onClicked: { root.command("new", {variant: newGame.variant, computer:[newWhite.currentIndex===1,newBlack.currentIndex===1]}); newGame.close(); } }
            }
        }
    }
    Kirigami.Dialog {
        id: promotionDialog
        title: "Promote your pawn"
        preferredWidth: 400
        visible: root.game.promotion.length > 0
        ColumnLayout {
            Controls.Label { text: "Choose the piece your pawn becomes."; color: "#6e776d" }
            RowLayout {
                Repeater { model: root.game.promotion; delegate: SoftButton { required property string modelData; text: modelData; onClicked: root.action("promote", {role: modelData}); } }
            }
            SoftButton { text: "Cancel"; quiet: true; onClicked: root.action("clear_selection"); }
        }
        onClosed: if (root.game.promotion.length) root.action("clear_selection");
    }
    Kirigami.Dialog {
        id: closeDialog
        title: "Save your game?"
        preferredWidth: 420
        visible: root.game.close_index !== null
        closePolicy: Controls.Popup.NoAutoClose
        ColumnLayout {
            Controls.Label { text: "This game has changes that haven’t been saved."; wrapMode: Text.WordWrap; Layout.fillWidth: true; color: "#6e776d" }
            RowLayout {
                SoftButton { text: "Cancel"; quiet: true; onClicked: root.action("close_response", {choice:"cancel"}); }
                SoftButton { text: "Discard"; onClicked: root.action("close_response", {choice:"discard"}); }
                SoftButton { text: "Save game"; primary: true; onClicked: {
                    const tab = root.game.tabs[root.game.close_index];
                    if (root.game.close_index === root.game.active && root.game.path) root.action("close_response", {choice:"save"});
                    else { root.savePurpose = "close"; saveDialog.open(); }
                } }
            }
        }
    }
    Kirigami.Dialog {
        id: offerDialog
        title: "Opponent’s offer"
        preferredWidth: 420
        visible: root.game.remote_request !== null
        closePolicy: Controls.Popup.NoAutoClose
        ColumnLayout {
            Controls.Label { text: "Your opponent offers a " + (root.game.remote_request || "") + "."; color: "#6e776d" }
            RowLayout {
                SoftButton { text: "Decline"; onClicked: root.command("respond", {accepted:false}); }
                SoftButton { text: "Accept"; primary: true; onClicked: root.command("respond", {accepted:true}); }
            }
        }
    }
    Kirigami.Dialog {
        id: resignDialog
        title: "Resign this game?"
        preferredWidth: 400
        ColumnLayout {
            Controls.Label { text: "Your opponent will win this game."; color: "#6e776d" }
            RowLayout { SoftButton { text: "Keep playing"; onClicked: resignDialog.close(); } SoftButton { text: "Resign"; onClicked: { root.command("resign"); resignDialog.close(); } } }
        }
    }
    Kirigami.Dialog {
        id: positionDialog
        title: "Set up a position"
        preferredWidth: 600
        ColumnLayout {
            Controls.Label { text: "Start a new game in the current variant using a FEN position."; wrapMode: Text.WordWrap; Layout.fillWidth: true; color: "#6e776d" }
            Controls.TextArea { id: fenInput; Layout.fillWidth: true; Layout.preferredHeight: 110; wrapMode: Text.WrapAnywhere; Accessible.name: "FEN position" }
            SoftButton { text: "Open position"; primary: true; onClicked: { if (root.command("set_fen", {fen:fenInput.text}).ok) positionDialog.close(); } }
        }
    }
    Kirigami.Dialog {
        id: infoDialog
        title: "Game information"
        preferredWidth: 550
        Kirigami.FormLayout {
            Repeater {
                model: ["Event","Site","Date","Round","White","Black","City","Country","StartTime"]
                delegate: Controls.TextField {
                    required property string modelData
                    Kirigami.FormData.label: modelData + ":"
                    Layout.fillWidth: true
                    text: root.game.headers[modelData] || ""
                    onEditingFinished: { const fields = {}; fields[modelData] = text; root.action("metadata", fields); }
                }
            }
        }
    }
    Kirigami.Dialog {
        id: networkDialog
        title: "Play together"
        preferredWidth: 500
        ColumnLayout {
            spacing: 14
            Controls.Label { text: "Host plays White. Your guest plays Black."; color: "#6e776d" }
            Controls.TextField { id: addressInput; Layout.fillWidth: true; text: root.game.network_address; placeholderText: "Opponent address or host bind address"; enabled: !root.game.network_active }
            RowLayout {
                visible: !root.game.network_active
                SoftButton { text: "Host current game"; primary: true; icon.name: "network-server"; onClicked: root.command("host", {address:addressInput.text}); }
                SoftButton { text: "Join game"; icon.name: "network-connect"; onClicked: root.command("join", {address:addressInput.text}); }
            }
            RowLayout {
                visible: root.game.network_active
                Controls.Label { text: root.game.connected ? "Connected" : "Waiting for a connection…"; Layout.fillWidth: true; color: "#3e5b48" }
                SoftButton { text: "Disconnect"; onClicked: root.command("disconnect"); }
            }
            Controls.ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: 180
                Controls.TextArea { readOnly: true; text: root.game.chat.join("\n"); wrapMode: Text.WordWrap; placeholderText: "Say hello to your opponent." }
            }
            RowLayout {
                Controls.TextField { id: chatInput; Layout.fillWidth: true; placeholderText: "Message your opponent"; onAccepted: sendChat.clicked(); }
                SoftButton { id: sendChat; text: "Send"; enabled: root.game.connected; onClicked: { root.action("chat", {text:chatInput.text}); chatInput.text=""; } }
            }
        }
    }
    Kirigami.Dialog {
        id: helpDialog
        title: "Chess, at your pace"
        preferredWidth: 580
        ColumnLayout {
            spacing: 18
            Controls.Label { text: "Click a piece, then its destination. Or drag it into place.\nRight-drag rotates the board. Scroll zooms. F flips your perspective."; wrapMode: Text.WordWrap; Layout.fillWidth: true }
            Controls.Label { text: "Ctrl+N  New game     Ctrl+O  Open     Ctrl+S  Save\nCtrl+Z  Take back      Ctrl+Shift+Z  Redo\n]  Suggest a move     [  Show last move     F11  Full screen\nArrow keys  Choose a square     Enter  Select / move     Escape  Clear"; wrapMode: Text.WordWrap; Layout.fillWidth: true; color: "#6e776d" }
            Controls.Label { text: "The command field accepts e2e4, Nf3, O-O, N@e4 and spoken phrases such as ‘knight to f three’. The 2D board exposes labelled squares to screen readers."; wrapMode: Text.WordWrap; Layout.fillWidth: true }
            Controls.Label { text: "Built with Rust, Kirigami and Vulkan.\nOriginal artwork and geometry: Apple Sample Code License.\nRust application: GPL-3.0-or-later. Noto fonts: SIL Open Font License."; wrapMode: Text.WordWrap; Layout.fillWidth: true; font.pixelSize: 11; color: "#788371" }
        }
    }
    FileDialog {
        id: openDialog
        title: "Open game"
        nameFilters: ["Chess games (*.chess-linux *.chess *.pgn *.json)", "All files (*)"]
        onAccepted: root.command("open", {path:root.path(selectedFile)});
    }
    FileDialog {
        id: saveDialog
        title: root.savePurpose === "record" ? "Record game" : root.savePurpose === "screenshot" ? "Save screenshot" : "Save game"
        fileMode: FileDialog.SaveFile
        nameFilters: root.savePurpose === "record" ? ["MP4 video (*.mp4)"] : root.savePurpose === "screenshot" ? ["PNG image (*.png)"] : ["Chess Linux (*.chess-linux)", "Apple Chess (*.chess)", "PGN game (*.pgn)"]
        onAccepted: {
            const file = root.path(selectedFile);
            if (root.savePurpose === "record") root.action("record", {path:file,size:[root.width,root.height]});
            else if (root.savePurpose === "screenshot") root.action("gui_screenshot", {path:file});
            else if (root.savePurpose === "close") root.action("close_response", {choice:"save",path:file});
            else root.command("save", {path:file});
        }
    }
    Shortcut { sequence: "Ctrl+N"; enabled: !root.game.network_active; onActivated: newGame.open(); }
    Shortcut { sequence: "Ctrl+O"; enabled: !root.game.network_active; onActivated: openDialog.open(); }
    Shortcut { sequence: "Ctrl+S"; onActivated: root.saveGame(false); }
    Shortcut { sequence: "Ctrl+Shift+S"; onActivated: root.saveGame(true); }
    Shortcut { sequence: "Ctrl+Z"; enabled: !root.editingText; onActivated: root.command("undo"); }
    Shortcut { sequence: "Ctrl+Shift+Z"; enabled: !root.editingText && !root.game.network_active && root.game.ply < root.game.total; onActivated: root.command("seek", {ply:root.game.ply+1}); }
    Shortcut { sequence: "Ctrl+Q"; onActivated: root.command("quit"); }
    Shortcut { sequence: "F"; enabled: !root.editingText; onActivated: root.action("flip"); }
    Shortcut { sequence: "]"; enabled: !root.editingText; onActivated: root.command("hint"); }
    Shortcut { sequence: "["; enabled: !root.editingText; onActivated: root.action("show_last", {enabled:true}); }
    Shortcut { sequence: "F11"; onActivated: root.visibility = root.visibility === Window.FullScreen ? Window.Windowed : Window.FullScreen; }
}
