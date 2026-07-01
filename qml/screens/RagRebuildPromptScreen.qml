import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Page {
    id: root
    background: Rectangle {
        color: Theme.p.background
    }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Your knowledge index needs rebuilding.")
    Accessible.description: qsTr("Re-scan local files. Nothing leaves your device.")
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.lg
        spacing: Spacing.md
        Text {
            Layout.fillWidth: true
            text: qsTr("Your knowledge index needs rebuilding.")
            color: Theme.p.inkPrimary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.h1)
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("Re-scan local files. Nothing leaves your device.")
            color: Theme.p.inkSecondary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
        PrimaryButton {
            text: qsTr("Continue")
        }
        GhostButton {
            text: qsTr("Close")
        }
    }
}
