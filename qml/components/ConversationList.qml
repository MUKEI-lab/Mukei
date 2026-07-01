import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

ListView {
    id: root
    property var conversations: []
    signal conversationSelected(string id)
    model: conversations
    Accessible.role: Accessible.List
    Accessible.name: qsTr("Conversation list")
    Accessible.description: qsTr("Recent conversations grouped by date")
    delegate: ItemDelegate { width: ListView.view.width; text: modelData.title || qsTr("Untitled conversation"); Accessible.name: qsTr("Open conversation"); Accessible.description: text; onClicked: root.conversationSelected(modelData.id || "") }
}
