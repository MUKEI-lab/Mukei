pragma Singleton
import QtQuick
import QtQml.Models

QtObject {
    property var agentSource: null
    property var conversations: ListModel { id: conversationModel; dynamicRoles: true }
    property bool hydrated: false
    readonly property int count: conversationModel.count

    signal hydrationCompleted
    signal conversationOpened(string conversationId, string branchId)

    function configure(agent) {
        agentSource = agent
    }

    function hydrate() {
        conversationModel.clear()
        if (agentSource === null || typeof agentSource.conversation_list_json !== "function") {
            hydrated = true
            hydrationCompleted()
            return
        }
        try {
            var value = JSON.parse(agentSource.conversation_list_json(100))
            if (value && value.error) {
                ErrorStore.push(value.error, "ERR_UI_CONVERSATION_SNAPSHOT")
            } else if (Array.isArray(value)) {
                for (var i = 0; i < value.length; ++i) {
                    var row = value[i]
                    conversationModel.append({
                        conversationId: row.conversation_id || "",
                        branchId: row.active_branch_id || "",
                        title: row.title || qsTr("Untitled conversation"),
                        preview: row.preview || "",
                        updatedAt: row.updated_at || ""
                    })
                }
            }
        } catch (error) {
            ErrorStore.push({
                code: "ERR_UI_CONVERSATION_SNAPSHOT",
                severity: "warning",
                recoverable: true,
                safe_message: qsTr("Recent conversations could not be restored.")
            })
        }
        hydrated = true
        hydrationCompleted()
    }

    function openConversation(conversationId, branchId) {
        if (!conversationId || !branchId)
            return false
        ChatStore.openConversation(conversationId, branchId)
        NavigationStore.navigate("chat", {
            conversationId: conversationId,
            branchId: branchId
        }, false)
        conversationOpened(conversationId, branchId)
        return true
    }
}
