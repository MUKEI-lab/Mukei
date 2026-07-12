pragma Singleton
import QtQuick

QtObject {
    id: root

    property var agentSource: null
    property var interruptedTurn: null
    property bool hydrated: false
    property string claimedOperationId: ""
    property bool loading: false
    property string lastRequestId: ""
    property string lastErrorCode: ""
    readonly property bool available: interruptedTurn !== null
    readonly property string conversationId: available ? (interruptedTurn.conversation_id || "") : ""
    readonly property string branchId: available ? (interruptedTurn.branch_id || "") : ""
    readonly property string partialText: available ? (interruptedTurn.generated_prefix || "") : ""

    signal changed
    signal recoveryClaimed(string operationId, string conversationId, string branchId)
    signal hydrationCompleted

    function configure(agent) {
        agentSource = agent
    }

    function finishHydration(payload) {
        interruptedTurn = payload || null
        loading = false
        hydrated = true
        lastErrorCode = ""
        changed()
        hydrationCompleted()
    }

    function failHydration(error) {
        loading = false
        hydrated = true
        lastErrorCode = error && error.code ? error.code : "ERR_UI_RECOVERY_SNAPSHOT"
        ErrorStore.push(error || ({
            code: "ERR_UI_RECOVERY_SNAPSHOT",
            severity: "warning",
            recoverable: true,
            safe_message: qsTr("An interrupted response could not be inspected.")
        }), "ERR_UI_RECOVERY_SNAPSHOT")
        hydrationCompleted()
    }

    function hydrate() {
        if (loading)
            return
        interruptedTurn = null
        claimedOperationId = ""
        hydrated = false
        if (agentSource === null || typeof agentSource.interrupted_turn_json !== "function") {
            finishHydration(null)
            return
        }
        loading = true
        try {
            var value = JSON.parse(agentSource.interrupted_turn_json())
            if (value && value.accepted === true) {
                lastRequestId = value.request_id || ""
                return
            }
            if (value && value.error)
                failHydration(value.error)
            else
                finishHydration(value)
        } catch (error) {
            failHydration(null)
        }
    }

    function markClaimed(operationId, conversationId, branchId) {
        if (!available || !operationId || claimedOperationId === operationId)
            return false
        claimedOperationId = operationId
        interruptedTurn = null
        changed()
        recoveryClaimed(operationId, conversationId || "", branchId || "")
        return true
    }

    function clearLocal() {
        interruptedTurn = null
        changed()
    }

    property Connections asyncResultConnections: Connections {
        target: root.agentSource
        ignoreUnknownSignals: true
        function onAsync_result(resultJson) {
            var result
            try { result = JSON.parse(resultJson) } catch (error) { return }
            if (!result || result.domain !== "recovery.snapshot"
                    || result.request_id !== root.lastRequestId
                    || result.current === false)
                return
            if (result.ok === true)
                root.finishHydration(result.payload)
            else
                root.failHydration(result.error)
        }
    }
}
