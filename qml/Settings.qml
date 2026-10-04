pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.Dialog {
    id: dialog
    required property var app
    property int selectedPage: 0
    readonly property var prefs: dialog.app.prefs
    title: "Preferences"
    preferredWidth: 760
    preferredHeight: 590
    property var boardNames: ["Wood", "Marble", "Metal", "Grass"]
    property var pieceNames: ["Wood", "Marble", "Metal", "Fur"]
    property var swatches: [["#dec797","#845132"],["#e8e6e1","#747c78"],["#c5c2b5","#626860"],["#bccca1","#4b6743"]]
    function view(patch) { dialog.app.view(patch); }
    function preference(patch) { dialog.app.preference(patch); }
    component SectionHeading: Controls.Label { font.family: "Noto Serif"; font.pixelSize: 22; color: "#252f28" }
    component Caption: Controls.Label { color: "#6e776d"; font.pixelSize: 11; wrapMode: Text.WordWrap; Layout.fillWidth: true }
    component StyleTile: SoftButton {
        id: tile
        property string styleName: "Wood"
        property var colors: ["#dec797", "#845132"]
        property bool pieces: false
        implicitHeight: 104
        implicitWidth: 102
        leftPadding: 8
        rightPadding: 8
        contentItem: ColumnLayout {
            spacing: 9
            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: 54
                Rectangle {
                    anchors.fill: parent
                    color: tile.colors[0]
                    radius: 5
                    Grid {
                        visible: !tile.pieces
                        anchors.centerIn: parent
                        columns: 4
                        Repeater { model: 12; delegate: Rectangle { required property int index; width: 17; height: 17; color: (Math.floor(index/4) + index%4)%2 ? tile.colors[1] : tile.colors[0] } }
                    }
                    Row {
                        visible: tile.pieces
                        anchors.centerIn: parent
                        spacing: 2
                        Controls.Label { text: "♟"; font.pixelSize: 31; color: tile.colors[1] }
                        Controls.Label { text: "♟"; font.pixelSize: 31; color: "#fffdf3"; style: Text.Outline; styleColor: tile.colors[1] }
                    }
                }
            }
            Controls.Label { text: tile.styleName; color: "#344c3c"; font.pixelSize: 12; Layout.alignment: Qt.AlignHCenter }
        }
    }
    RowLayout {
        spacing: 22
        ColumnLayout {
            Layout.preferredWidth: 142
            Layout.fillHeight: true
            spacing: 6
            Repeater {
                model: [
                    {name:"Appearance", icon:"draw-brush"},
                    {name:"Computer", icon:"computer"},
                    {name:"Speech", icon:"audio-input-microphone"},
                    {name:"Lighting", icon:"color-management"}
                ]
                delegate: SoftButton {
                    required property var modelData
                    required property int index
                    Layout.fillWidth: true
                    text: modelData.name
                    icon.name: modelData.icon
                    checked: dialog.selectedPage === index
                    quiet: !checked
                    onClicked: dialog.selectedPage = index
                }
            }
            Item { Layout.fillHeight: true }
            Caption { text: "Make yourself at home.\nSettings save automatically."; font.pixelSize: 10 }
        }
        Rectangle { implicitWidth: 1; Layout.fillHeight: true; color: "#e1e2d8" }
        Controls.ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: 440
            clip: true
            Controls.ScrollBar.horizontal.policy: Controls.ScrollBar.AlwaysOff
            StackLayout {
                width: parent.availableWidth
                currentIndex: dialog.selectedPage
                ColumnLayout {
                    spacing: 16
                    SectionHeading { text: "Your board, your style" }
                    Caption { text: "Choose the materials and a perspective that feels right." }
                    Controls.Label { text: "Board material"; font.weight: Font.Medium }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        Repeater { model: dialog.boardNames; delegate: StyleTile { required property string modelData; required property int index; Layout.fillWidth: true; styleName: modelData; colors: dialog.swatches[index]; checked: dialog.prefs.view.board_style === index; onClicked: dialog.view({board_style: index}); } }
                    }
                    Controls.Label { text: "Piece material"; font.weight: Font.Medium }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        Repeater { model: dialog.pieceNames; delegate: StyleTile { required property string modelData; required property int index; Layout.fillWidth: true; styleName: modelData; pieces: true; colors: dialog.swatches[index]; checked: dialog.prefs.view.piece_style === index; onClicked: dialog.view({piece_style: index}); } }
                    }
                    Kirigami.FormLayout {
                        Layout.fillWidth: true
                        Controls.Slider { Kirigami.FormData.label: "Board angle:"; from: 20; to: 89; value: dialog.prefs.view.elevation; onMoved: dialog.view({elevation: value}); Layout.fillWidth: true }
                        Controls.Slider { Kirigami.FormData.label: "Rotation:"; from: 0; to: 360; value: (dialog.prefs.view.yaw % 360 + 360) % 360; onMoved: dialog.view({yaw: value}); Layout.fillWidth: true }
                    }
                    Controls.CheckBox { text: "Show board coordinates"; checked: dialog.prefs.view.coordinates; onClicked: dialog.view({coordinates: checked}); }
                    Controls.CheckBox { text: "Animate moves"; checked: dialog.prefs.view.animations; onClicked: dialog.view({animations: checked}); }
                    Controls.CheckBox { text: "2D board with accessible square buttons"; checked: dialog.prefs.view.flat; onClicked: dialog.view({flat: checked}); }
                    Item { Layout.fillHeight: true }
                }
                ColumnLayout {
                    spacing: 16
                    SectionHeading { text: "A worthy opponent" }
                    Caption { text: "Set who plays each side and how long the computer thinks." }
                    Kirigami.FormLayout {
                        Layout.fillWidth: true
                        Controls.ComboBox { Kirigami.FormData.label: "White:"; model: ["Human", "Computer"]; currentIndex: dialog.app.game.computer[0] ? 1 : 0; enabled: !dialog.app.game.network_active; onActivated: dialog.app.action("computer", {computer:[currentIndex===1,dialog.app.game.computer[1]]}); }
                        Controls.ComboBox { Kirigami.FormData.label: "Black:"; model: ["Human", "Computer"]; currentIndex: dialog.app.game.computer[1] ? 1 : 0; enabled: !dialog.app.game.network_active; onActivated: dialog.app.action("computer", {computer:[dialog.app.game.computer[0],currentIndex===1]}); }
                        RowLayout {
                            Kirigami.FormData.label: "Thinking time:"
                            Controls.Slider { Layout.fillWidth: true; from: 0.05; to: 30; value: dialog.prefs.seconds; onMoved: dialog.preference({seconds:value}); }
                            Controls.Label { text: dialog.prefs.seconds.toFixed(1) + " s"; color: "#6e776d"; Layout.preferredWidth: 40 }
                        }
                        RowLayout {
                            Kirigami.FormData.label: "Search depth:"
                            Controls.Slider { Layout.fillWidth: true; from: 1; to: 16; stepSize: 1; value: dialog.prefs.depth; onMoved: dialog.preference({depth:value}); }
                            Controls.Label { text: dialog.prefs.depth.toString(); color: "#6e776d"; Layout.preferredWidth: 40 }
                        }
                    }
                    SoftButton { text: dialog.app.game.paused ? "Resume computer play" : "Pause computer play"; icon.name: dialog.app.game.paused ? "media-playback-start" : "media-playback-pause"; enabled: !dialog.app.game.network_active; onClicked: dialog.app.command("pause", {paused:!dialog.app.game.paused}); }
                    Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: "#e1e2d8" }
                    Controls.Label { text: "Original Sjeng engine"; font.weight: Font.Medium }
                    Caption { text: "Use the supplied engine’s original search, opening books and learning. Leave the path empty for the Rust engine." }
                    Controls.TextField { Layout.fillWidth: true; text: dialog.prefs.sjeng_path; placeholderText: "Optional Sjeng executable path"; onEditingFinished: dialog.preference({sjeng_path:text}); }
                    Caption { text: "Build it with scripts/build-sjeng.sh, then select target/sjeng/sjeng." }
                    Controls.CheckBox { text: "Log engine analysis to the terminal"; checked: dialog.prefs.engine_log; onClicked: dialog.preference({engine_log:checked}); }
                    Item { Layout.fillHeight: true }
                }
                ColumnLayout {
                    spacing: 16
                    SectionHeading { text: "A game you can hear" }
                    Caption { text: "Speak moves aloud or play with your voice. Recognition stays on your machine." }
                    Controls.CheckBox { text: "Speak computer and opponent moves"; checked: dialog.prefs.speak_computer; onClicked: dialog.preference({speak_computer:checked}); }
                    Controls.CheckBox { text: "Speak human moves"; checked: dialog.prefs.speak_human; onClicked: dialog.preference({speak_human:checked}); }
                    Kirigami.FormLayout {
                        Layout.fillWidth: true
                        Controls.TextField { Kirigami.FormData.label: "White voice:"; text: dialog.prefs.voices[0]; onEditingFinished: dialog.preference({voices:[text,dialog.prefs.voices[1]]}); Layout.fillWidth: true }
                        Controls.TextField { Kirigami.FormData.label: "Black voice:"; text: dialog.prefs.voices[1]; onEditingFinished: dialog.preference({voices:[dialog.prefs.voices[0],text]}); Layout.fillWidth: true }
                    }
                    Caption { text: "Spoken output uses espeak voice identifiers, such as en or en+f3." }
                    Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: "#e1e2d8" }
                    Controls.Label { text: "Offline voice recognition"; font.weight: Font.Medium }
                    Controls.TextField { Layout.fillWidth: true; text: dialog.prefs.model; placeholderText: "Vosk model folder"; onEditingFinished: dialog.preference({model:text}); }
                    Caption { text: "Requires Python, vosk, sounddevice and an extracted Vosk model. Try ‘knight to f three’ or ‘take back move’." }
                    SoftButton { text: dialog.app.game.listening ? "Stop listening" : "Listen for spoken moves"; icon.name: "audio-input-microphone"; primary: true; onClicked: dialog.app.action("listen", {enabled:!dialog.app.game.listening}); }
                    Item { Layout.fillHeight: true }
                }
                ColumnLayout {
                    spacing: 16
                    SectionHeading { text: "Set the mood" }
                    Caption { text: "Fine-tune reflections, light and each surface material. Changes appear on the board immediately." }
                    Kirigami.FormLayout {
                        Layout.fillWidth: true
                        Controls.Slider { Kirigami.FormData.label: "Reflections:"; Layout.fillWidth: true; from: 0; to: 1; value: dialog.prefs.view.reflectivity; onMoved: dialog.view({reflectivity:value}); }
                        Controls.Slider { Kirigami.FormData.label: "Ambient light:"; Layout.fillWidth: true; from: 0; to: 1; value: dialog.prefs.view.ambient; onMoved: dialog.view({ambient:value}); }
                        Controls.Slider { Kirigami.FormData.label: "Notation brightness:"; Layout.fillWidth: true; from: 0; to: 1; value: dialog.prefs.view.label_intensity; onMoved: dialog.view({label_intensity:value}); }
                    }
                    Controls.Label { text: "Light position"; font.weight: Font.Medium }
                    RowLayout {
                        Repeater {
                            model: ["X","Y","Z"]
                            delegate: ColumnLayout {
                                required property string modelData
                                required property int index
                                Controls.Label { text: modelData; color: "#6e776d" }
                                Controls.SpinBox { from: -30; to: 30; editable: true; value: dialog.prefs.view.light[index]; onValueModified: { const lights=dialog.prefs.view.light.slice(); lights[index]=value; dialog.view({light:lights}); } }
                            }
                        }
                    }
                    Controls.ComboBox { id: material; Layout.fillWidth: true; model: ["White pieces","Black pieces","White squares","Black squares","Border"] }
                    Kirigami.FormLayout {
                        Layout.fillWidth: true
                        Repeater {
                            model: [{name:"Diffuse",key:"diffuse",max:2},{name:"Specular",key:"specular",max:2},{name:"Shininess",key:"shininess",max:200},{name:"Opacity",key:"alpha",max:1}]
                            delegate: Controls.Slider {
                                required property var modelData
                                Kirigami.FormData.label: modelData.name + ":"
                                Layout.fillWidth: true
                                from: modelData.key === "shininess" ? 1 : 0
                                to: modelData.max
                                value: dialog.prefs.view.materials[material.currentIndex][modelData.key]
                                onMoved: { const materials=JSON.parse(JSON.stringify(dialog.prefs.view.materials)); materials[material.currentIndex][modelData.key]=value; dialog.view({materials:materials}); }
                            }
                        }
                    }
                    SoftButton { text: "Reset lighting and materials"; onClicked: dialog.view({ambient:0.38,reflectivity:0.25,label_intensity:0.9,light:[-6,12,8],materials:Array.from({length:5},()=>({diffuse:0.68,specular:0.55,shininess:35,alpha:1}))}); }
                    Item { Layout.fillHeight: true }
                }
            }
        }
    }
}
