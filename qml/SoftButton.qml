import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Controls.Button {
    id: control
    property bool primary: false
    property bool quiet: false
    font.family: "Noto Sans"
    font.pixelSize: 13
    font.weight: Font.Medium
    implicitHeight: 38
    implicitWidth: Math.max(36, contentRow.implicitWidth + leftPadding + rightPadding)
    leftPadding: 14
    rightPadding: 14
    icon.width: 18
    icon.height: 18
    icon.color: primary ? "#fffef8" : "#3e5b48"
    palette.buttonText: primary ? "#fffef8" : "#344c3c"
    contentItem: RowLayout {
        id: contentRow
        spacing: 8
        Kirigami.Icon {
            visible: control.icon.name.length > 0
            source: control.icon.name
            implicitWidth: 18
            implicitHeight: 18
            color: control.primary ? "#fffef8" : "#3e5b48"
            opacity: control.enabled ? 1 : 0.4
            Layout.alignment: Qt.AlignHCenter
        }
        Controls.Label {
            visible: control.text.length > 0
            text: control.text
            font: control.font
            color: control.primary ? "#fffef8" : "#344c3c"
            opacity: control.enabled ? 1 : 0.4
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            elide: Text.ElideRight
        }
    }
    background: Rectangle {
        radius: 8
        color: control.primary ? (control.down ? "#2e4435" : control.hovered ? "#4b6c55" : "#3e5b48") :
               control.checked ? "#e4ebdd" : control.hovered ? "#e9ede3" : control.quiet ? "transparent" : "#fffefa"
        border.color: control.primary || control.quiet ? "transparent" : control.checked ? "#a6b89e" : "#dedfd4"
        border.width: 1
        opacity: control.enabled ? 1 : 0.4
        Rectangle {
            anchors.fill: parent
            anchors.margins: -3
            radius: 11
            color: "transparent"
            border.width: 2
            border.color: "#8d9f7d"
            visible: control.visualFocus
        }
    }
    Controls.ToolTip.visible: hovered && text.length === 0
    Controls.ToolTip.text: Accessible.name
    Controls.ToolTip.delay: 600
}
