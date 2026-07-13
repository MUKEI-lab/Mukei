import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Page {
    id: root

    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Interrupted response recovery")

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(parent.width - Spacing.xl * 2, Spacing.huge * 7)
        spacing: Spacing.lg

        MukeiIcon {
            Layout.alignment: Qt.AlignHCenter
            name: "regenerate"
            size: 32
            tone: Theme.p.accent
        }

        Text {
            Layout.fillWidth: true
            text: qsTr("A response was interrupted")
            color: Theme.p.inkPrimary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.h2)
        }

        Text {
            Layout.fillWidth: true
            text: qsTr("Your partial response is safe. Continue from where it stopped, create a fresh attempt, or return without changing it.")
            color: Theme.p.inkSecondary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }

        Rectangle {
            Layout.fillWidth: true
            visible: RecoveryStore.partialText.length > 0
            implicitHeight: partialText.implicitHeight + Spacing.lg * 2
            radius: Theme.radiusLg
            color: Theme.p.surface
            border.width: 1
            border.color: Theme.p.divider

            Text {
                id: partialText
                anchors.fill: parent
                anchors.margins: Spacing.lg
                text: RecoveryStore.partialText
                color: Theme.p.inkPrimary
                wrapMode: Text.Wrap
                maximumLineCount: 8
                elide: Text.ElideRight
                Component.onCompleted: Type.apply(this, Type.bodyAI)
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Spacing.sm

            PrimaryButton {
                Layout.fillWidth: true
                text: qsTr("Continue")
                onClicked: IntentDispatcher.dispatch({ type: "recovery.resume" })
            }

            SecondaryButton {
                Layout.fillWidth: true
                text: qsTr("Start again")
                onClicked: IntentDispatcher.dispatch({ type: "recovery.regenerate" })
            }

            GhostButton {
                Layout.fillWidth: true
                text: qsTr("Not now")
                onClicked: IntentDispatcher.dispatch({ type: "recovery.dismiss" })
            }
        }
    }
}
