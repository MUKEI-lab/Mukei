import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"

Drawer {
    id: root

    width: Math.min(parent ? parent.width * 0.88 : 360, 408)
    height: parent ? parent.height : implicitHeight
    edge: Qt.LeftEdge
    modal: true
    interactive: true

    background: Rectangle {
        color: Theme.p.surface
        border.width: Theme.highContrast ? 1 : 0
        border.color: Theme.p.divider
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: Spacing.lg
        anchors.rightMargin: Spacing.lg
        anchors.topMargin: Spacing.lg
        anchors.bottomMargin: Spacing.lg
        spacing: Spacing.md

        RowLayout {
            Layout.fillWidth: true
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                Text {
                    text: qsTr("Mukei")
                    color: Theme.p.inkPrimary
                    Component.onCompleted: Type.apply(this, Type.h2)
                }
                Text {
                    text: qsTr("Private, on this device")
                    color: Theme.p.inkSecondary
                    Component.onCompleted: Type.apply(this, Type.caption)
                }
            }
            IconButton {
                iconSource: "qrc:/icons/close.svg"
                text: qsTr("Close drawer")
                onClicked: root.close()
            }
        }

        PrimaryButton {
            Layout.fillWidth: true
            text: qsTr("New conversation")
            enabled: CapabilityStore.canClearConversation || CapabilityStore.canSendMessage
            onClicked: {
                if (CapabilityStore.canClearConversation)
                    IntentDispatcher.dispatch({ type: "chat.clearConversation" })
                IntentDispatcher.dispatch({ type: "navigation.open", route: "chat" })
                root.close()
            }
        }

        SearchField { Layout.fillWidth: true }

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

        RowLayout {
            Layout.fillWidth: true
            spacing: Spacing.xs

            GhostButton {
                Layout.fillWidth: true
                text: qsTr("Models")
                onClicked: {
                    IntentDispatcher.dispatch({ type: "navigation.open", route: "models" })
                    root.close()
                }
            }
            GhostButton {
                Layout.fillWidth: true
                text: qsTr("Settings")
                onClicked: {
                    IntentDispatcher.dispatch({ type: "navigation.open", route: "settings" })
                    root.close()
                }
            }
        }

        StatusPill {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Local-only")
            subtype: "Network-Offline"
            iconSource: "qrc:/icons/lock.svg"
        }
    }
}
