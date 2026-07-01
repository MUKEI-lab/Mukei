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
        RowLayout {
            PrimaryButton {
                text: qsTr("Rebuild now")
            }
            GhostButton {
                text: qsTr("Skip for now")
            }
        }
        Item {
            Layout.fillHeight: true
        }
    }
}
