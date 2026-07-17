import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../architecture"
import "../theme"
import "../components"

Page {
    id: root
    objectName: "ragRebuildPromptScreen"
    objectName: "ragRebuildPromptScreen"

    background: Rectangle {
        color: Theme.p.background
    }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("RAG rebuild prompt")
    Accessible.description: qsTr("Rebuild the private knowledge index")

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.xl
        spacing: Spacing.lg
        Item {
            Layout.preferredHeight: Spacing.xl
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("Your knowledge index needs rebuilding.")
            color: Theme.p.inkPrimary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.h1)
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("Mukei found that the local index of your private files is no longer compatible. Rebuilding will re-scan only files you've shared with Mukei — nothing leaves your device.")
            color: Theme.p.inkSecondary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("Rebuild is not available in this runtime yet. No indexing action will be simulated or started silently.")
            color: Theme.p.inkSecondary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodySmall)
        }
        RowLayout {
            PrimaryButton {
                objectName: "ragRebuildUnavailableButton"
                // interaction-audit: exempt — awaiting a supported local rebuild operation.
                text: qsTr("Rebuild unavailable")
                enabled: false
            }
            GhostButton {
                objectName: "ragRebuildSkipButton"
                text: qsTr("Skip for now")
                onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })
            }
        }
        Item {
            Layout.fillHeight: true
        }
    }
}
