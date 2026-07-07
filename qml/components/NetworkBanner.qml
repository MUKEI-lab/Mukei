import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Rectangle {
    id: root
    property bool online: false
    property string text: online ? qsTr("Network: on") : qsTr("🔒 local-only · Network: off — you are private")
    Accessible.role: Accessible.StaticText
    Accessible.name: qsTr("Network privacy status")
    Accessible.description: text
    color: "transparent"
    implicitHeight: row.implicitHeight + Spacing.xs
    RowLayout {
        id: row
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        spacing: Spacing.xs
        Image {
            source: root.online ? "qrc:/icons/network-on.svg" : "qrc:/icons/network-off.svg"
            Layout.preferredWidth: Spacing.md
            Layout.preferredHeight: Spacing.md
        }
        Text {
            text: root.text
            color: root.online ? Theme.p.inkSecondary : Theme.success
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.caption)
        }
    }
}
