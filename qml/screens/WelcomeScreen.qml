import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Page {
    id: root
    signal getStarted
    background: Rectangle {
        color: Theme.p.background
    }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Welcome screen")
    Accessible.description: qsTr("Introduction to private on-device AI")
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.xl
        spacing: Spacing.xl
        Item {
            Layout.preferredHeight: Spacing.huge
        }
        Text {
            text: qsTr("Your Private AI,\nOn Your Device.")
            color: Theme.p.inkPrimary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.display)
        }
        Text {
            text: qsTr("No cloud. No subscriptions.\nNo data leaves your phone.")
            color: Theme.p.inkSecondary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
        Text {
            text: qsTr("🔒 Encrypted locally with your device.")
            color: Theme.p.accent
            Component.onCompleted: Type.apply(this, Type.bodySmall)
        }
        Item {
            Layout.fillHeight: true
        }
        PrimaryButton {
            Layout.fillWidth: true
            text: qsTr("Get Started")
            onClicked: root.getStarted()
        }
    }
}
