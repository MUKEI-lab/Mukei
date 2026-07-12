pragma Singleton
import QtQuick
import QtQml.Models

QtObject {
    property var agentSource: null
    property var operations: ListModel { id: operationModel; dynamicRoles: true }
    readonly property int totalCount: operationModel.count
    readonly property int activeCount: {
        var count = 0
        for (var i = 0; i < operationModel.count; ++i)
            if (!isTerminalState(operationModel.get(i).state)
                    && operationModel.get(i).state !== "blocked")
                ++count
        return count
    }
    readonly property int blockedCount: {
        var count = 0
        for (var i = 0; i < operationModel.count; ++i)
            if (operationModel.get(i).state === "blocked")
                ++count
        return count
    }
    readonly property bool hasActiveOperations: activeCount > 0
    property bool hydrated: false

    signal operationChanged(string operationId)
    signal operationStarted(string operationId, string commandType, var scope)
    signal operationTerminal(string operationId, string commandType, string state, var result, var error)
    signal hydrationCompleted

    function configure(agent) { agentSource = agent }

    function isTerminalState(state) {
        return ["completed", "failed", "cancelled", "rejected"].indexOf(state) >= 0
    }

    function findById(operationId) {
        for (var i = 0; i < operationModel.count; ++i)
            if (operationModel.get(i).operationId === operationId)
                return i
        return -1
    }

    function findByRequestId(requestId) {
        for (var i = 0; i < operationModel.count; ++i)
            if (operationModel.get(i).requestId === requestId)
                return i
        return -1
    }

    function labelForCommand(commandType) {
        switch (commandType) {
        case "app.initialize": return qsTr("Starting Mukei")
        case "chat.send_message": return qsTr("Sending message")
        case "chat.stop_generation": return qsTr("Stopping response")
        case "chat.clear_conversation": return qsTr("Clearing conversation")
        case "model.download": return qsTr("Downloading model")
        case "download.cancel": return qsTr("Stopping model download")
        case "model.select": return qsTr("Selecting model")
        case "model.delete": return qsTr("Deleting model")
        case "document.grant": return qsTr("Adding private document")
        case "document.revoke": return qsTr("Removing private document")
        case "document.retry_ingestion": return qsTr("Retrying document indexing")
        case "settings.update": return qsTr("Saving preference")
        case "recovery.resume": return qsTr("Resuming interrupted response")
        case "recovery.regenerate": return qsTr("Restarting interrupted response")
        default: return qsTr("Working…")
        }
    }

    function normalise(operation) {
        var commandType = operation.commandType || operation.command_type || ""
        return {
            operationId: operation.operationId || operation.operation_id || operation.type || "operation",
            commandId: operation.commandId || operation.command_id || "",
            requestId: operation.requestId || operation.request_id || "",
            correlationId: operation.correlationId || operation.correlation_id || "",
            commandType: commandType,
            type: operation.type || operation.operationKind || operation.operation_kind || commandType || "unknown",
            scope: operation.scope && typeof operation.scope === "object" ? operation.scope : ({}),
            state: operation.state || "active",
            commandState: operation.commandState || operation.command_state || operation.state || "active",
            phase: operation.phase || "",
            progress: typeof operation.progress === "number" ? Math.max(0, Math.min(1, operation.progress)) : 0,
            cancelable: operation.cancelable === true,
            retryable: operation.retryable === true,
            label: operation.label || labelForCommand(commandType),
            safeMessage: operation.safeMessage || operation.safe_message || operation.latestSafeError || "",
            latestSafeError: operation.latestSafeError || operation.latest_safe_error || operation.safeMessage || operation.safe_message || "",
            relatedEntityId: operation.relatedEntityId || operation.related_entity_id || "",
            createdAt: operation.createdAt || operation.created_at || "",
            acceptedAt: operation.acceptedAt || operation.accepted_at || "",
            startedAt: operation.startedAt || operation.started_at || "",
            terminalAt: operation.terminalAt || operation.terminal_at || "",
            updatedAt: operation.updatedAt || operation.updated_at || "",
            latestResult: typeof operation.latestResult !== "undefined" ? operation.latestResult : null
        }
    }

    function mergeRow(existing, incoming) {
        var row = Object.assign({}, existing, incoming)
        if (isTerminalState(existing.state)) {
            // First terminal projection wins. Replayed or late events must not rewrite a
            // completed/failed/cancelled/rejected operation into a different terminal state.
            row.state = existing.state
            row.commandState = existing.commandState
            row.terminalAt = existing.terminalAt
            row.updatedAt = existing.updatedAt
            row.latestSafeError = existing.latestSafeError
            row.safeMessage = existing.safeMessage
            row.latestResult = existing.latestResult
        }
        return normalise(row)
    }

    function upsert(operation) {
        var row = normalise(operation)
        var index = findById(row.operationId)
        if (index < 0)
            operationModel.append(row)
        else
            operationModel.set(index, mergeRow(operationModel.get(index), row))
        operationChanged(row.operationId)
    }

    function beginCommand(command) {
        if (!command || !command.request_id)
            return false
        var provisionalId = command.operation_id || ("request:" + command.request_id)
        if (findByRequestId(command.request_id) >= 0)
            return true
        upsert({
            operationId: provisionalId,
            commandId: command.command_id || "",
            requestId: command.request_id,
            correlationId: command.correlation_id || "",
            commandType: command.command_type || "",
            type: command.command_type || "command",
            scope: command.scope || ({}),
            state: "created",
            commandState: "created",
            createdAt: command.submitted_at || new Date().toISOString(),
            label: labelForCommand(command.command_type || "")
        })
        return true
    }

    function markAwaitingAcknowledgement(requestId) {
        var index = findByRequestId(requestId)
        if (index < 0)
            return false
        var row = operationModel.get(index)
        if (!isTerminalState(row.state)) {
            row.state = "awaiting_acknowledgement"
            row.commandState = "awaiting_acknowledgement"
            row.updatedAt = new Date().toISOString()
            operationModel.set(index, normalise(row))
            operationChanged(row.operationId)
        }
        return true
    }

    function applyAcknowledgement(ack, command) {
        if (!ack || typeof ack !== "object")
            return false
        var requestId = ack.request_id || (command ? command.request_id : "")
        var index = findByRequestId(requestId)
        if (index < 0 && command) {
            beginCommand(command)
            index = findByRequestId(requestId)
        }
        if (index < 0)
            return false

        var row = operationModel.get(index)
        var status = ack.status || "rejected"
        if (status === "rejected") {
            if (!isTerminalState(row.state)) {
                row.state = "rejected"
                row.commandState = "rejected"
                row.terminalAt = ack.timestamp || new Date().toISOString()
                row.latestSafeError = ack.rejection_reason || "command_rejected"
                row.safeMessage = row.latestSafeError
                row.updatedAt = row.terminalAt
                operationModel.set(index, normalise(row))
                operationChanged(row.operationId)
                operationTerminal(row.operationId, row.commandType, row.state, null, ({ reason: ack.rejection_reason || "command_rejected" }))
            }
            return false
        }

        var operationId = ack.operation_id || row.operationId
        var existingOperationIndex = findById(operationId)
        if (existingOperationIndex >= 0 && existingOperationIndex !== index) {
            // An idempotent resubmission can acknowledge the original operation identity with a
            // new request_id. Keep one operation row and discard only the replay's provisional row.
            var existingOperation = operationModel.get(existingOperationIndex)
            existingOperation.commandId = ack.command_id || existingOperation.commandId
            existingOperation.correlationId = ack.correlation_id || existingOperation.correlationId
            existingOperation.acceptedAt = existingOperation.acceptedAt || ack.timestamp || new Date().toISOString()
            existingOperation.updatedAt = existingOperation.updatedAt || existingOperation.acceptedAt
            operationModel.set(existingOperationIndex, normalise(existingOperation))
            operationModel.remove(index)
            operationChanged(operationId)
            return true
        }
        if (operationId !== row.operationId)
            row.operationId = operationId
        row.commandId = ack.command_id || row.commandId
        row.requestId = ack.request_id || row.requestId
        row.correlationId = ack.correlation_id || row.correlationId
        row.acceptedAt = ack.timestamp || row.acceptedAt || new Date().toISOString()
        row.updatedAt = row.acceptedAt
        if (!isTerminalState(row.state) && row.state !== "running") {
            row.state = "accepted"
            row.commandState = "accepted"
        }
        operationModel.set(index, normalise(row))
        operationChanged(row.operationId)
        return true
    }



    function markCancellationRequested(operationId) {
        var index = findById(operationId)
        if (index < 0)
            return false
        var row = operationModel.get(index)
        if (isTerminalState(row.state))
            return false
        row.state = "cancelling"
        row.commandState = "cancelling"
        row.phase = "cancelling"
        row.updatedAt = new Date().toISOString()
        operationModel.set(index, normalise(row))
        operationChanged(operationId)
        return true
    }

    function remove(operationId) {
        var index = findById(operationId)
        if (index >= 0)
            operationModel.remove(index)
    }

    function removeByType(type) {
        for (var i = operationModel.count - 1; i >= 0; --i)
            if (operationModel.get(i).type === type)
                operationModel.remove(i)
    }

    function hydrate() {
        var protocolRows = []
        for (var keep = 0; keep < operationModel.count; ++keep) {
            var existing = operationModel.get(keep)
            if (existing.commandId || existing.requestId)
                protocolRows.push(normalise(existing))
        }
        operationModel.clear()
        if (agentSource !== null && typeof agentSource.operation_snapshot_json === "function") {
            try {
                var snapshot = JSON.parse(agentSource.operation_snapshot_json())
                var rows = snapshot && Array.isArray(snapshot.operations) ? snapshot.operations : []
                for (var i = 0; i < rows.length; ++i)
                    upsert(rows[i])
                if (snapshot && snapshot.error)
                    ErrorStore.push(snapshot.error, "ERR_UI_OPERATION_SNAPSHOT")
            } catch (error) {
                ErrorStore.push({
                    code: "ERR_UI_OPERATION_SNAPSHOT", severity: "warning", recoverable: true,
                    safe_message: qsTr("Background operation status could not be restored.")
                }, "ERR_UI_OPERATION_SNAPSHOT")
                reconcileDurableState()
            }
        } else {
            reconcileDurableState()
        }
        for (var p = 0; p < protocolRows.length; ++p)
            upsert(protocolRows[p])
        hydrated = true
        hydrationCompleted()
    }

    function reconcileDurableState() {
        for (var i = 0; i < DownloadStore.jobs.count; ++i) {
            var job = DownloadStore.jobs.get(i)
            if (["queued", "starting", "downloading", "cancelling"].indexOf(job.state) >= 0) {
                upsert({
                    operationId: "download:" + (job.modelId || job.jobId),
                    type: "download", state: job.state, progress: job.progress,
                    cancelable: CapabilityStore.canStopDownload, retryable: false,
                    label: qsTr("Downloading model"), relatedEntityId: job.modelId
                })
            }
        }
        for (var j = 0; j < DocumentStore.documents.count; ++j) {
            var document = DocumentStore.documents.get(j)
            if (document.cleanupPending === true) {
                upsert({
                    operationId: "document_cleanup:" + document.documentId,
                    type: "document_cleanup", state: "pending", progress: 0,
                    cancelable: false, retryable: true,
                    label: qsTr("Cleaning private document data"), relatedEntityId: document.documentId
                })
            } else if (["queued", "reading", "chunking", "embedding", "committing", "waiting_for_embedder"].indexOf(document.ingestionState) >= 0) {
                var blocked = document.ingestionState === "waiting_for_embedder"
                upsert({
                    operationId: "document_ingestion:" + document.documentId,
                    type: "document_ingestion", state: blocked ? "blocked" : document.ingestionState,
                    phase: document.ingestionState, progress: document.ingestionProgress / 100,
                    cancelable: false, retryable: document.ingestionRetryable,
                    label: blocked ? qsTr("Waiting for document embedder") : qsTr("Indexing private document"),
                    safeMessage: document.ingestionError, relatedEntityId: document.documentId
                })
            }
        }
    }

    function eventState(event) {
        if (event.category === "operation_lifecycle")
            return event.state
        if (event.category === "chat_completed")
            return "completed"
        if (event.category === "chat_cancelled")
            return "cancelled"
        if (event.category === "chat_failed")
            return "failed"
        if (event.category === "chat_state") {
            if (["completed", "cancelled", "failed"].indexOf(event.state) >= 0)
                return event.state
            return "running"
        }
        if (event.category === "download_completed")
            return "completed"
        if (event.category === "download_failed")
            return "failed"
        if (event.category === "download_state") {
            if (["completed", "cancelled", "failed"].indexOf(event.state) >= 0)
                return event.state
            return "running"
        }
        if (event.category === "download_progress")
            return "running"
        if (event.category === "app_lifecycle") {
            if (["ready", "degraded"].indexOf(event.state) >= 0)
                return "completed"
            if (event.state === "fatal_error")
                return "failed"
            return "running"
        }
        if (event.category === "error")
            return "failed"
        return "running"
    }

    function applyCorrelatedEvent(event) {
        var operationId = event.operation_id || ""
        if (!operationId)
            return false
        var index = findById(operationId)
        if (index < 0 && event.request_id)
            index = findByRequestId(event.request_id)
        if (index < 0) {
            upsert({
                operationId: operationId,
                commandId: event.command_id || "",
                requestId: event.request_id || "",
                correlationId: event.correlation_id || "",
                commandType: event.command_type || "",
                type: event.command_type || event.category,
                state: "accepted",
                commandState: "accepted",
                acceptedAt: event.emitted_at || event.timestamp || "",
                scope: ({
                    conversation_id: event.conversation_id || "",
                    branch_id: event.branch_id || "",
                    turn_id: event.turn_id || "",
                    model_id: event.model_id || "",
                    document_id: event.document_id || ""
                })
            })
            index = findById(operationId)
        }
        if (index < 0)
            return false

        var row = operationModel.get(index)
        var eventConversation = event.conversation_id || ""
        var eventBranch = event.branch_id || ""
        if (row.scope && typeof row.scope === "object") {
            if (row.scope.conversation_id && eventConversation
                    && row.scope.conversation_id !== eventConversation)
                return false
            if (row.scope.branch_id && eventBranch
                    && row.scope.branch_id !== eventBranch)
                return false
        }
        var nextState = eventState(event)
        if (isTerminalState(row.state))
            return true

        var wasRunning = row.state === "running"
        row.operationId = operationId
        row.commandId = event.command_id || row.commandId
        row.requestId = event.request_id || row.requestId
        row.correlationId = event.correlation_id || row.correlationId
        row.commandType = event.command_type || row.commandType
        row.type = row.commandType || row.type
        row.state = nextState
        row.commandState = nextState === "running" ? "accepted" : nextState
        row.phase = event.state || event.category || row.phase
        row.updatedAt = event.emitted_at || event.timestamp || new Date().toISOString()
        if (typeof event.progress === "number")
            row.progress = Math.max(0, Math.min(1, event.progress))
        if (event.error) {
            row.latestSafeError = event.error.user_message || event.error.safe_message || event.error.code || ""
            row.safeMessage = row.latestSafeError
        }
        if (typeof event.result !== "undefined" && event.result !== null)
            row.latestResult = event.result
        if (nextState === "running" && !row.startedAt)
            row.startedAt = row.updatedAt
        if (isTerminalState(nextState) && !row.terminalAt)
            row.terminalAt = row.updatedAt
        operationModel.set(index, normalise(row))
        operationChanged(operationId)

        if (nextState === "running" && !wasRunning)
            operationStarted(operationId, row.commandType, row.scope)
        if (isTerminalState(nextState))
            operationTerminal(operationId, row.commandType, nextState, event.result || null, event.error || null)
        return true
    }

    function applyLegacyEvent(event) {
        if (event.category === "download_state") {
            var id = event.model_id ? "download:" + event.model_id : "download:active"
            if (["completed", "cancelled", "failed", "idle"].indexOf(event.state) >= 0) {
                remove(id)
                return
            }
            upsert({ operationId: id, type: "download", state: event.state, progress: 0,
                     cancelable: event.capabilities && event.capabilities.can_stop_download === true,
                     label: qsTr("Preparing model download"), relatedEntityId: event.model_id || "" })
        } else if (event.category === "download_progress") {
            var progressId = event.model_id ? "download:" + event.model_id : "download:active"
            upsert({ operationId: progressId, type: "download", state: event.state,
                     progress: event.progress, cancelable: true, label: qsTr("Downloading model"),
                     relatedEntityId: event.model_id || "" })
        } else if (event.category === "download_completed") {
            for (var i = operationModel.count - 1; i >= 0; --i)
                if (operationModel.get(i).type === "download" && !operationModel.get(i).commandId)
                    operationModel.remove(i)
        }
    }

    function applyEvent(event) {
        if (!event)
            return
        if (!applyCorrelatedEvent(event))
            applyLegacyEvent(event)
    }
}
