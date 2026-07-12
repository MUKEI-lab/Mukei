import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

import "../architecture"
import "../stores"
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
            enabled: CapabilityStore.canClearConversation || CapabilityStore.canSendMessage
            onClicked: {
                if (CapabilityStore.canClearConversation)
                    IntentDispatcher.dispatch({ type: "chat.clearConversation" })
                root.close()
            }
        }
        SearchField {
            Layout.fillWidth: true
        }
        ConversationList {
            Layout.fillWidth: true
            Layout.fillHeight: true
            onConversationSelected: function(conversationId, branchId) {
                IntentDispatcher.dispatch({
                    type: "conversation.open",
                    conversationId: conversationId,
                    branchId: branchId
                })
                root.close()
            }
        }
        GhostButton {
            text: qsTr("Open Settings")
            enabled: CapabilityStore.canOpenSettings
            onClicked: {
                IntentDispatcher.dispatch({ type: "navigation.open", route: "settings" })
                root.close()
            }
        }
    }
}
