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
    Accessible.name: qsTr("Model manager")
    Accessible.description: qsTr("Manage installed and available models.")
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.lg
        spacing: Spacing.md
        Text {
            Layout.fillWidth: true
            text: qsTr("Model manager")
            color: Theme.p.inkPrimary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.h1)
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("Manage installed and available models.")
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
