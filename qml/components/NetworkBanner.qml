import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Rectangle {
    id: root
    property bool remoteAllowed: false
    property string text: remoteAllowed ? qsTr("Remote features are allowed by your privacy setting") : qsTr("🔒 Local-only mode · data stays on this device")
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
            source: root.remoteAllowed ? "qrc:/icons/network-on.svg" : "qrc:/icons/network-off.svg"
            Layout.preferredWidth: Spacing.md
            Layout.preferredHeight: Spacing.md
        }
        Text {
            text: root.text
            color: root.remoteAllowed ? Theme.warning : Theme.success
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.caption)
        }
    }
}
