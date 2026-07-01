import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Drawer {
    id: root
    width: Type.compact ? Spacing.huge * 3 - Spacing.xs : Spacing.huge * 3 + Spacing.xl
    edge: Qt.LeftEdge
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Conversation drawer")
    Accessible.description: qsTr("New chat, conversation list, and settings")
    background: Rectangle { color: Theme.p.surface; border.width: Theme.highContrast ? 1 : 0; border.color: Theme.p.divider }
    ColumnLayout { anchors.fill: parent; anchors.margins: Spacing.md; spacing: Spacing.md; PrimaryButton { text: qsTr("New Chat") } SearchField { Layout.fillWidth: true } ConversationList { Layout.fillWidth: true; Layout.fillHeight: true } GhostButton { text: qsTr("Open Settings") } }
}
