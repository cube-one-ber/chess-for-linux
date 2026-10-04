import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: card
    property string playerName: "White player"
    property string detail: "Human · White"
    property bool whiteSide: true
    property bool toMove: false
    implicitHeight: 74
    radius: 12
    color: toMove ? "#e7eddf" : "#fffefa"
    border.width: 1
    border.color: toMove ? "#c5d0ba" : "#e1e2d8"
    RowLayout {
        anchors.fill: parent
        anchors.margins: 13
        spacing: 12
        Rectangle {
            Layout.preferredWidth: 44
            Layout.preferredHeight: 44
            radius: 22
            color: card.whiteSide ? "#fffef8" : "#303e33"
            border.color: card.whiteSide ? "#dcded2" : "#303e33"
            Controls.Label {
                anchors.centerIn: parent
                text: card.whiteSide ? "♔" : "♚"
                font.pixelSize: 29
                color: card.whiteSide ? "#3c4a3d" : "#faf8ed"
            }
        }
        ColumnLayout {
            spacing: 3
            Layout.fillWidth: true
            Controls.Label { text: card.playerName; font.pixelSize: 14; font.weight: Font.Medium; color: "#252f28"; elide: Text.ElideRight; Layout.fillWidth: true }
            Controls.Label { text: card.detail; font.pixelSize: 11; color: "#6e776d" }
        }
        Rectangle { implicitWidth: 7; implicitHeight: 7; radius: 4; color: "#58714b"; visible: card.toMove }
    }
}
