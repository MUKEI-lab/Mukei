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
    Accessible.name: qsTr("Safe mode")
    Accessible.description: qsTr("Recover calmly from repeated crashes")

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.xl
        spacing: Spacing.lg

        Item {
            Layout.preferredHeight: Spacing.xl
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("We've had a few crashes.
What now?")
            color: Theme.p.inkPrimary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.display)
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("Mukei detected 2 unexpected closures in the last 24 hours. You can continue anyway, or reset all data to start fresh. Your model file will be kept either way.")
            color: Theme.p.inkSecondary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
        PrimaryButton {
            Layout.fillWidth: true
            text: qsTr("Continue Anyway")
        }
        DestructiveButton {
            Layout.fillWidth: true
            text: qsTr("Reset All Data")
        }
        GhostButton {
            text: qsTr("View Crash Log")
        }
        Item {
            Layout.fillHeight: true
        }
    }
}
