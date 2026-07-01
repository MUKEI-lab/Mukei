import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Page {
    id: root
    property bool composerStreaming: false
    property ListModel chatModel: ListModel {
        id: timelineModel
        ListElement {
            type: "user_message"
            text: "what is entropy?"
            phase: ""
            kind: ""
        }
        ListElement {
            type: "timeline_event"
            text: "Thinking"
            phase: "result"
            kind: "thinking"
        }
        ListElement {
            type: "assistant_message"
            text: "Entropy is a measure of uncertainty or disorder in a system."
            phase: ""
            kind: ""
        }
        ListElement {
            type: "timeline_event"
            text: "Searching web…"
            phase: "active"
            kind: "tool"
        }
    }
    signal accessibilityAnnouncementRequested(string text)
    background: Rectangle {
        color: Theme.p.background
    }
    Keys.onEscapePressed: if (composerStreaming)
        mukeiAgent.stop_generation()
    LeftDrawer {
        id: drawer
    }
    Connections {
        target: mukeiAgent
        function onChunk_generated(chunk) {
            timelineModel.append({
                    "type": "assistant_message",
                    "text": chunk,
                    "phase": "",
                    "kind": ""
                });
        }
        function onStream_finalized() {
            root.composerStreaming = false;
        }
        function onThinking_started() {
            timelineModel.append({
                    "type": "timeline_event",
                    "text": qsTr("Thinking"),
                    "phase": "active",
                    "kind": "thinking"
                });
        }
        function onThinking_completed() {
            timelineModel.append({
                    "type": "timeline_event",
                    "text": qsTr("Thinking complete"),
                    "phase": "result",
                    "kind": "thinking"
                });
        }
        function onTool_call_started(toolName) {
            timelineModel.append({
                    "type": "timeline_event",
                    "text": toolName,
                    "phase": "active",
                    "kind": "tool"
                });
        }
        function onTool_call_completed(toolName, result) {
            timelineModel.append({
                    "type": "timeline_event",
                    "text": toolName + qsTr(" complete"),
                    "phase": "result",
                    "kind": "tool"
                });
        }
        function onState_changed(state) {
            root.composerStreaming = state === "streaming";
        }
        function onError_occurred(code, msg) {
            timelineModel.append({
                    "type": "timeline_event",
                    "text": code + qsTr(": ") + msg,
                    "phase": "failure",
                    "kind": "system"
                });
        }
    }
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.md
        spacing: Spacing.sm
        RowLayout {
            Layout.fillWidth: true
            IconButton {
                id: drawerButton
                iconSource: "qrc:/icons/chat.svg"
                Accessible.name: qsTr("Open drawer")
                onClicked: drawer.open()
            }
            Text {
                Layout.fillWidth: true
                text: qsTr("Mukei")
                color: Theme.p.inkPrimary
                Component.onCompleted: Type.apply(this, Type.h3)
            }
            IconButton {
                id: settingsButton
                iconSource: "qrc:/icons/settings.svg"
                Accessible.name: qsTr("Open settings")
            }
        }
        Flickable {
            id: chatFlickable
            objectName: "chatFlickable"
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: width
            contentHeight: timeline.implicitHeight
            clip: true
            ColumnLayout {
                id: timeline
                objectName: "timeline"
                width: chatFlickable.width
                spacing: Spacing.lg
                Repeater {
                    model: timelineModel
                    delegate: Loader {
                        Layout.fillWidth: true
                        sourceComponent: model.type === "user_message" ? userBubble : model.type === "assistant_message" ? aiBubble : timelineEvent
                        property string entryText: model.text
                        property string entryPhase: model.phase
                        property string entryKind: model.kind
                    }
                }
            }
        }
        NetworkBanner {
            Layout.fillWidth: true
            online: false
        }
        ChatComposer {
            id: composer
            Layout.fillWidth: true
            isStreaming: root.composerStreaming
            onSendRequested: function (text) {
                root.composerStreaming = true;
                mukeiAgent.send_message(text);
            }
            onStopRequested: mukeiAgent.stop_generation()
        }
    }
    Component {
        id: userBubble
        UserMessageBubble {
            Layout.fillWidth: true
            text: entryText
            timestamp: qsTr("Now")
        }
    }
    Component {
        id: aiBubble
        AIMessageBubble {
            Layout.fillWidth: true
            text: entryText
            timestamp: qsTr("Now")
        }
    }
    Component {
        id: timelineEvent
        ChatTimelineEvent {
            Layout.fillWidth: true
            kind: entryKind
            label: entryText
            phase: entryPhase
            iconSource: entryKind === "tool" ? "qrc:/icons/search.svg" : ""
        }
    }
}
