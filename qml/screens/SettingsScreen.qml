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
    Accessible.name: qsTr("Settings")
    Accessible.description: qsTr("General · Privacy · Storage · About")
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.lg
        spacing: Spacing.md
        Text {
            Layout.fillWidth: true
            text: qsTr("Settings")
            color: Theme.p.inkPrimary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.h1)
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("General · Privacy · Storage · About")
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
