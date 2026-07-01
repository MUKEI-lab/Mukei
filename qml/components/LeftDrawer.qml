import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Drawer {
    id: root
    width: Type.compact ? Spacing.huge * 3 - Spacing.xs : Spacing.huge * 3 + Spacing.xl
    edge: Qt.LeftEdge
    background: Rectangle {
        color: Theme.p.surface
        border.width: Theme.highContrast ? 1 : 0
        border.color: Theme.p.divider
    }
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.md
        spacing: Spacing.md
        PrimaryButton {
            text: qsTr("New Chat")
        }
        SearchField {
            Layout.fillWidth: true
        }
        ConversationList {
            Layout.fillWidth: true
            Layout.fillHeight: true
        }
        GhostButton {
            text: qsTr("Open Settings")
        }
    }
}
