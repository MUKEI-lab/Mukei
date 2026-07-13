import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../stores"
import "../theme"

ListView {
    id: root
    property var conversations: ConversationStore.conversations
    signal conversationSelected(string conversationId, string branchId)
    model: conversations
    clip: true
    spacing: Spacing.xs
    Accessible.role: Accessible.List
    Accessible.name: qsTr("Conversation list")
    Accessible.description: qsTr("Recent private conversations")

    delegate: ItemDelegate {
        required property string conversationId
        required property string branchId
        required property string title
        required property string preview
        width: ListView.view.width
        implicitHeight: contentColumn.implicitHeight + Spacing.md * 2
        Accessible.name: titleText.text
        Accessible.description: previewText.text
        onClicked: root.conversationSelected(conversationId || "", branchId || "")

        contentItem: ColumnLayout {
            id: contentColumn
            spacing: Spacing.xs
            Text {
                id: titleText
                Layout.fillWidth: true
                text: title || qsTr("Untitled conversation")
                color: Theme.p.inkPrimary
                elide: Text.ElideRight
                Component.onCompleted: Type.apply(this, Type.bodyUI)
            }
            Text {
                id: previewText
                Layout.fillWidth: true
                text: preview || qsTr("Private conversation")
                color: Theme.p.inkFaint
                elide: Text.ElideRight
                Component.onCompleted: Type.apply(this, Type.caption)
            }
        }
    }

    Text {
        anchors.centerIn: parent
        visible: root.count === 0
        text: qsTr("No previous conversations")
        color: Theme.p.inkFaint
        Component.onCompleted: Type.apply(this, Type.caption)
    }
}
