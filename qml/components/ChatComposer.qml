import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

FocusScope {
    id: root
    objectName: "chatComposer"

    property alias text: textArea.text
    property alias cursorPosition: textArea.cursorPosition
    property bool isStreaming: false
    property bool canSend: true
    signal sendRequested(string text)
    signal stopRequested
    signal attachRequested

    function forceEditorFocus() {
        textArea.forceActiveFocus()
    }

    Accessible.role: Accessible.EditableText
    Accessible.name: qsTr("Compose message")
    Accessible.description: qsTr("Type a private message to Mukei")
    implicitHeight: Math.max(56,
                             Math.min(textArea.contentHeight + Spacing.md * 2,
                                      Type.bodyUI.pixelSize * 6 + Spacing.lg))

    Rectangle {
        anchors.fill: parent
        radius: Theme.radiusXl
        color: Theme.p.surface
        border.width: textArea.activeFocus || Theme.highContrast ? 2 : 1
        border.color: textArea.activeFocus ? Theme.p.accent : Theme.p.divider

        Behavior on border.color {
            ColorAnimation { duration: Theme.reduceMotion ? 0 : Motion.microTransition }
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Spacing.sm
        anchors.rightMargin: Spacing.sm
        anchors.topMargin: Spacing.xs
        anchors.bottomMargin: Spacing.xs
        spacing: Spacing.xs

        IconButton {
            objectName: "chatAttachButton"
            iconSource: "qrc:/icons/attach.svg"
            text: qsTr("Attach local file")
            Accessible.description: qsTr("Add a private local document to this conversation")
            onClicked: root.attachRequested()
        }

        TextArea {
            id: textArea
            objectName: "chatMessageEditor"
            Layout.fillWidth: true
            Layout.minimumHeight: Type.bodyUI.pixelSize
            Layout.maximumHeight: Type.bodyUI.pixelSize * 6
            wrapMode: TextArea.Wrap
            color: Theme.p.inkPrimary
            selectionColor: Qt.rgba(Theme.p.accent.r, Theme.p.accent.g, Theme.p.accent.b, 0.22)
            selectedTextColor: Theme.p.inkPrimary
            placeholderText: qsTr("Ask Mukei anything…")
            placeholderTextColor: Theme.p.inkFaint
            background: null
            Accessible.name: qsTr("Message text")
            Accessible.description: qsTr("One to six line message editor")
            Component.onCompleted: Type.apply(this, Type.bodyUI)

            Keys.onPressed: function(event) {
                if ((event.modifiers & (Qt.ControlModifier | Qt.MetaModifier))
                        && event.key === Qt.Key_Return) {
                    if (!root.isStreaming && root.canSend && textArea.text.trim().length > 0)
                        root.sendRequested(textArea.text)
                    event.accepted = true
                }
            }
        }

        Button {
            id: sendButton
            objectName: "chatSendButton"
            implicitWidth: Spacing.xxl
            implicitHeight: Spacing.xxl
            enabled: root.isStreaming || (root.canSend && textArea.text.trim().length > 0)
            Accessible.name: root.isStreaming ? qsTr("Stop response") : qsTr("Send message")
            onClicked: root.isStreaming ? root.stopRequested() : root.sendRequested(textArea.text)

            background: Rectangle {
                radius: width / 2
                color: sendButton.enabled ? Theme.p.accent : Theme.p.surfaceVariant
                scale: sendButton.down && !Theme.reduceMotion ? 0.94 : 1
                Behavior on scale {
                    NumberAnimation { duration: Motion.immediateFeedback; easing.type: Easing.OutCubic }
                }
            }

            contentItem: MukeiIcon {
                name: root.isStreaming ? "stop" : "send"
                tone: sendButton.enabled ? Theme.p.background : Theme.p.inkFaint
            }
        }
    }
}
