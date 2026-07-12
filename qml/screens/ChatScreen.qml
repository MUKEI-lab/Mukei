import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Qt.labs.qmlmodels
import "../theme"
import "../components"

import "../architecture"
import "../stores"
Page {
    id: root
    property bool followTail: true
    property bool unseenTailUpdate: false
    property real contentHeightBeforePrepend: -1
    signal accessibilityAnnouncementRequested(string text)

    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Chat")

    Keys.onEscapePressed: {
        if (ChatStore.streaming)
            IntentDispatcher.dispatch({ type: "chat.stopGeneration" })
    }

    LeftDrawer { id: drawer }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            Layout.preferredWidth: 300
            Layout.fillHeight: true
            visible: ResponsiveStore.expanded
            color: Theme.p.surface
            border.width: 1
            border.color: Theme.p.divider

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: Spacing.md
                spacing: Spacing.md

                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Conversations")
                        color: Theme.p.inkPrimary
                        Component.onCompleted: Type.apply(this, Type.h3)
                    }
                    IconButton {
                        iconSource: "qrc:/icons/chat.svg"
                        text: qsTr("New conversation")
                        enabled: CapabilityStore.canClearConversation || CapabilityStore.canSendMessage
                        onClicked: IntentDispatcher.dispatch({ type: "chat.clearConversation" })
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
                    }
                }
            }
        }

        ColumnLayout {
            id: chatPane
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: Spacing.md
            spacing: Spacing.sm

            RowLayout {
                Layout.fillWidth: true
                IconButton {
                    visible: !ResponsiveStore.expanded
                    iconSource: "qrc:/icons/chat.svg"
                    Accessible.name: qsTr("Open drawer")
                    onClicked: drawer.open()
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Mukei")
                        color: Theme.p.inkPrimary
                        Component.onCompleted: Type.apply(this, Type.h3)
                    }
                    Text {
                        Layout.fillWidth: true
                        visible: ChatStore.activeConversationId.length > 0
                        text: ChatStore.streaming ? qsTr("Responding privately on device") : qsTr("Private local conversation")
                        color: Theme.p.inkFaint
                        elide: Text.ElideRight
                        Component.onCompleted: Type.apply(this, Type.caption)
                    }
                }
                IconButton {
                    iconSource: "qrc:/icons/settings.svg"
                    enabled: CapabilityStore.canOpenSettings
                    Accessible.name: qsTr("Open settings")
                    onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "settings" })
                }
            }

            ListView {
                id: timelineView
                objectName: "chatTimelineView"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                spacing: Spacing.lg
                model: ChatStore.timeline
                cacheBuffer: Math.max(height, Spacing.huge * 6)
                boundsBehavior: Flickable.StopAtBounds
                reuseItems: true

                header: Item {
                    width: timelineView.width
                    height: ChatStore.hasOlderMessages ? loadOlderButton.implicitHeight + Spacing.md : 0
                    visible: ChatStore.hasOlderMessages
                    GhostButton {
                        id: loadOlderButton
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: ChatStore.olderPageLoading ? qsTr("Loading…") : qsTr("Load earlier messages")
                        enabled: !ChatStore.olderPageLoading
                        onClicked: {
                            root.contentHeightBeforePrepend = timelineView.contentHeight
                            IntentDispatcher.dispatch({ type: "chat.loadOlder" })
                        }
                    }
                }

                onMovementEnded: {
                    root.followTail = atYEnd
                    if (root.followTail)
                        root.unseenTailUpdate = false
                }
                onCountChanged: {
                    if (root.followTail)
                        Qt.callLater(positionViewAtEnd)
                    else
                        root.unseenTailUpdate = true
                }

                delegate: DelegateChooser {
                    role: "type"
                    DelegateChoice {
                        roleValue: "user_message"
                        delegate: UserMessageBubble {
                            width: timelineView.width
                            text: model.text
                            timestamp: model.timestamp
                        }
                    }
                    DelegateChoice {
                        roleValue: "assistant_message"
                        delegate: AIMessageBubble {
                            width: timelineView.width
                            text: model.text
                            timestamp: model.timestamp
                        }
                    }
                    DelegateChoice {
                        roleValue: "timeline_event"
                        delegate: ChatTimelineEvent {
                            width: timelineView.width
                            label: model.text
                            phase: model.phase
                            kind: model.kind
                            iconSource: model.kind === "tool" ? "qrc:/icons/search.svg" : ""
                        }
                    }
                }

                Text {
                    anchors.centerIn: parent
                    visible: timelineView.count === 0 && !ChatStore.snapshotLoading
                    text: qsTr("Your private conversation starts here.")
                    color: Theme.p.inkFaint
                    horizontalAlignment: Text.AlignHCenter
                    Component.onCompleted: Type.apply(this, Type.bodyUI)
                }

                BusyIndicator {
                    anchors.centerIn: parent
                    visible: ChatStore.snapshotLoading
                    running: visible
                }
            }

            GhostButton {
                Layout.alignment: Qt.AlignHCenter
                visible: root.unseenTailUpdate
                text: qsTr("Show latest response")
                onClicked: {
                    root.followTail = true
                    root.unseenTailUpdate = false
                    timelineView.positionViewAtEnd()
                }
            }

            NetworkBanner {
                Layout.fillWidth: true
                remoteAllowed: SettingsStore.remotePolicy === "remote_allowed"
            }

            ChatComposer {
                id: composer
                Layout.fillWidth: true
                isStreaming: ChatStore.streaming
                canSend: CapabilityStore.canSendMessage && !ChatStore.streaming
                text: ChatStore.draft
                cursorPosition: Math.min(ChatStore.draftCursorPosition, text.length)
                onTextChanged: {
                    if (text !== ChatStore.draft)
                        IntentDispatcher.dispatch({
                            type: "chat.updateDraft",
                            text: text,
                            cursorPosition: cursorPosition
                        })
                }
                onCursorPositionChanged: {
                    if (text === ChatStore.draft && cursorPosition !== ChatStore.draftCursorPosition)
                        IntentDispatcher.dispatch({
                            type: "chat.updateDraft",
                            text: text,
                            cursorPosition: cursorPosition
                        })
                }
                onSendRequested: function (message) {
                    IntentDispatcher.dispatch({ type: "chat.sendMessage", text: message })
                }
                onStopRequested: IntentDispatcher.dispatch({ type: "chat.stopGeneration" })
            }
        }
    }

    Connections {
        target: ChatStore
        function onTailUpdated() {
            if (root.followTail)
                Qt.callLater(timelineView.positionViewAtEnd)
            else
                root.unseenTailUpdate = true
        }
        function onDraftChangedByStore(value, cursorPosition) {
            if (composer.text !== value)
                composer.text = value
            composer.cursorPosition = Math.min(cursorPosition, composer.text.length)
        }
        function onSnapshotApplied() {
            if (root.contentHeightBeforePrepend >= 0) {
                var delta = timelineView.contentHeight - root.contentHeightBeforePrepend
                timelineView.contentY += Math.max(0, delta)
                root.contentHeightBeforePrepend = -1
            } else if (root.followTail) {
                Qt.callLater(timelineView.positionViewAtEnd)
            }
        }
    }

    Component.onCompleted: {
        if (NavigationStore.currentParameters.conversationId
                && NavigationStore.currentParameters.branchId
                && (ChatStore.activeConversationId !== NavigationStore.currentParameters.conversationId
                    || ChatStore.activeBranchId !== NavigationStore.currentParameters.branchId)) {
            ChatStore.openConversation(
                        NavigationStore.currentParameters.conversationId,
                        NavigationStore.currentParameters.branchId)
        } else {
            ChatStore.restoreDraft()
        }
    }
}
