import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

FocusScope {
    id: root

    property alias text: textArea.text
    property alias cursorPosition: textArea.cursorPosition
    property bool isStreaming: false
    property bool canSend: true
    signal sendRequested(string text)
    signal stopRequested
    signal attachRequested

    Accessible.role: Accessible.EditableText
    Accessible.name: qsTr("Compose message")
    Accessible.description: qsTr("Type a private message to Mukei")
    implicitHeight: Math.min(textArea.contentHeight + Spacing.md * 2, Type.bodyUI.pixelSize * 6 + Spacing.lg)

    Rectangle {
        anchors.fill: parent
        radius: Theme.radiusLg
        color: Theme.p.surface
        border.width: 2
        border.color: textArea.activeFocus ? Theme.p.accent : "transparent"
    }

    RowLayout {
        anchors.fill: parent
        anchors.margins: Spacing.md
        spacing: Spacing.sm

        IconButton {
            iconSource: "qrc:/icons/attach.svg"
            Accessible.name: qsTr("Attach file")
            Accessible.description: qsTr("Add a local file to this chat")
            onClicked: root.attachRequested()
        }

        TextArea {
            id: textArea
            Layout.fillWidth: true
            Layout.minimumHeight: Type.bodyUI.pixelSize
            Layout.maximumHeight: Type.bodyUI.pixelSize * 6
            wrapMode: TextArea.Wrap
            color: Theme.p.inkPrimary
            placeholderText: qsTr("Ask Mukei anything…")
            placeholderTextColor: Theme.p.inkFaint
            background: null
            Accessible.name: qsTr("Message text")
            Accessible.description: qsTr("One to six line message editor")
            Component.onCompleted: Type.apply(this, Type.bodyUI)
            Keys.onPressed: function (event) {
                if ((event.modifiers & (Qt.ControlModifier | Qt.MetaModifier)) && event.key === Qt.Key_Return) {
                    if (!root.isStreaming && root.canSend && textArea.text.trim().length > 0)
                        root.sendRequested(textArea.text);
                    event.accepted = true;
                }
            }
        }

        IconButton {
            iconSource: root.isStreaming ? "qrc:/icons/stop.svg" : "qrc:/icons/send.svg"
            enabled: root.isStreaming ? true : (root.canSend && textArea.text.trim().length > 0)
            Accessible.name: root.isStreaming ? qsTr("Stop response") : qsTr("Send message")
            Accessible.description: root.isStreaming ? qsTr("Stop the current model response") : qsTr("Send this message to Mukei")
            onClicked: root.isStreaming ? root.stopRequested() : root.sendRequested(textArea.text)
        }
    }
}
