pragma Singleton
import QtQuick
import "../stores"

QtObject {
    property var agentSource: null
    property var bridgeSource: null
    property var runtimeSource: null
    property int protocolCounter: 0

    // Keep protocol-critical dependencies injectable so isolated QuickTest
    // harnesses can preserve production semantics without relying on implicit
    // cross-directory QML name resolution. Production defaults still resolve
    // the singleton types from the compiled com.mukei.app module.
    property var contractStoreRef: typeof ContractStore !== "undefined" ? ContractStore : null
    property var capabilityStoreRef: typeof CapabilityStore !== "undefined" ? CapabilityStore : null
    property var chatStoreRef: typeof ChatStore !== "undefined" ? ChatStore : null
    property var operationStoreRef: typeof OperationStore !== "undefined" ? OperationStore : null

    signal intentAccepted(string type)
    signal intentRejected(string code, string message, var intent)

    function configure(agent, bridge, runtime) {
        agentSource = agent
        bridgeSource = bridge
        runtimeSource = runtime
    }

    function configureProtocolDependencies(contractStore, capabilityStore, chatStore, operationStore) {
        contractStoreRef = contractStore || null
        capabilityStoreRef = capabilityStore || null
        chatStoreRef = chatStore || null
        operationStoreRef = operationStore || null
    }

    function publishError(error, fallbackCode) {
        if (typeof ErrorStore !== "undefined"
                && ErrorStore !== null
                && typeof ErrorStore.push === "function")
            ErrorStore.push(error, fallbackCode)
    }

    function reject(code, message, intent) {
        intentRejected(code, message, intent)
        publishError({
            code: code,
            severity: "warning",
            recoverable: true,
            user_message: message
        }, code)
        return false
    }

    function opaqueId(prefix) {
        protocolCounter += 1
        var randomPart = Math.floor(Math.random() * 0x7fffffff).toString(36)
        return prefix + ":" + Date.now().toString(36) + ":" + protocolCounter.toString(36) + ":" + randomPart
    }

    function rejectionMessage(reason) {
        switch (reason) {
        case "unsupported_protocol": return qsTr("This action uses an unsupported local protocol version.")
        case "unknown_command": return qsTr("This action is not supported by the local service.")
        case "invalid_payload": return qsTr("The action contained invalid data and was not started.")
        case "capability_unavailable": return qsTr("That action is not available right now.")
        case "busy_conflict": return qsTr("Mukei is busy with a conflicting operation.")
        case "stale_scope": return qsTr("The action referred to state that is no longer current.")
        case "backend_unavailable": return qsTr("The local service is not ready.")
        case "duplicate_replay_conflict": return qsTr("A replay key was reused for a different action.")
        case "policy_denied": return qsTr("Local privacy or safety policy blocked that action.")
        default: return qsTr("The local service rejected that action.")
        }
    }

    function validAcknowledgement(acknowledgement, command) {
        if (!acknowledgement || typeof acknowledgement !== "object")
            return false
        if (!acknowledgement.protocol_version || Number(acknowledgement.protocol_version.major) !== 2)
            return false
        if (acknowledgement.command_id !== command.command_id
                || acknowledgement.request_id !== command.request_id
                || acknowledgement.correlation_id !== command.correlation_id)
            return false
        if (["accepted", "rejected"].indexOf(acknowledgement.status) < 0)
            return false
        if (acknowledgement.status === "accepted")
            return typeof acknowledgement.operation_id === "string"
                    && acknowledgement.operation_id.length > 0
                    && acknowledgement.operation_id.length <= 128
        return typeof acknowledgement.rejection_reason === "string"
                && acknowledgement.rejection_reason.length > 0
                && acknowledgement.rejection_reason.length <= 96
    }

    function syntheticRejection(command, reason) {
        return {
            protocol_version: { major: 2, minor: 0 },
            command_id: command.command_id,
            request_id: command.request_id,
            correlation_id: command.correlation_id,
            status: "rejected",
            rejection_reason: reason || "backend_unavailable",
            timestamp: new Date().toISOString()
        }
    }

    function submitBackendCommand(commandType, payload, scope, options) {
        options = options || ({})
        if (agentSource === null || typeof agentSource.submit_command_json !== "function") {
            reject("ERR_UI_PROTOCOL_UNAVAILABLE", qsTr("The reliable local command protocol is unavailable."), ({ type: commandType }))
            return ({ accepted: false })
        }
        if (operationStoreRef === null) {
            reject("ERR_UI_DISPATCH_DEPENDENCY", qsTr("The local operation tracker is unavailable."), ({ type: commandType }))
            return ({ accepted: false })
        }

        var requestId = opaqueId("request")
        var command = {
            protocol_version: { major: 2, minor: 0 },
            command_id: opaqueId("command"),
            request_id: requestId,
            command_type: commandType,
            submitted_at: new Date().toISOString(),
            correlation_id: opaqueId("correlation"),
            idempotency_key: options.idempotencyKey || opaqueId("idempotency"),
            payload: payload || ({})
        }
        if (scope && typeof scope === "object" && Object.keys(scope).length > 0)
            command.scope = scope
        if (options.operationId)
            command.operation_id = options.operationId

        operationStoreRef.beginCommand(command)
        operationStoreRef.markAwaitingAcknowledgement(requestId)

        var acknowledgement
        try {
            acknowledgement = JSON.parse(String(agentSource.submit_command_json(JSON.stringify(command))))
        } catch (error) {
            acknowledgement = syntheticRejection(command, "backend_unavailable")
        }
        if (!validAcknowledgement(acknowledgement, command))
            acknowledgement = syntheticRejection(command, "backend_unavailable")

        var accepted = operationStoreRef.applyAcknowledgement(acknowledgement, command)
        if (!accepted || acknowledgement.status !== "accepted") {
            var reason = acknowledgement.rejection_reason || "backend_unavailable"
            publishError({
                code: "ERR_COMMAND_REJECTED_" + String(reason).toUpperCase(),
                severity: "warning",
                recoverable: true,
                user_message: rejectionMessage(reason),
                operation_id: acknowledgement.operation_id || ""
            }, "ERR_UI_COMMAND_REJECTED")
            return ({ accepted: false, acknowledgement: acknowledgement, command: command })
        }
        return ({ accepted: true, acknowledgement: acknowledgement, command: command })
    }

    function dispatch(intent) {
        if (!intent || typeof intent !== "object" || typeof intent.type !== "string")
            return reject("ERR_UI_INVALID_INTENT", qsTr("That action was not valid."), intent)

        // Navigation is local presentation state and never depends on the
        // native command protocol being available.
        if (intent.type === "navigation.open") {
            if (!NavigationStore.navigate(intent.route, intent.parameters || ({}), intent.replace === true))
                return false
            intentAccepted(intent.type)
            return true
        }
        if (intent.type === "navigation.back") {
            if (!NavigationStore.goBack())
                return false
            intentAccepted(intent.type)
            return true
        }

        if (contractStoreRef === null || capabilityStoreRef === null || chatStoreRef === null || operationStoreRef === null)
            return reject("ERR_UI_DISPATCH_DEPENDENCY", qsTr("The local UI state machine is not ready."), intent)

        try {
            if (!contractStoreRef.compatible && intent.type !== "contract.retry")
                return reject("ERR_UI_CONTRACT_INCOMPATIBLE", contractStoreRef.safeMessage, intent)
            switch (intent.type) {
            case "contract.retry":
                if (!AppCoordinator.retryContractNegotiation())
                    return false
                break
            case "app.initialize": {
                if (agentSource === null)
                    return reject("ERR_UI_AGENT_UNAVAILABLE", qsTr("The local AI service is not available."), intent)
                var configPath = intent.configPath
                        || (runtimeSource && runtimeSource.configPath ? runtimeSource.configPath : "")
                if (configPath.length === 0)
                    return reject("ERR_UI_CONFIG_PATH", qsTr("The private configuration location is unavailable."), intent)
                if (!submitBackendCommand("app.initialize", ({ config_path: configPath }), ({})).accepted)
                    return false
                break
            }
            case "conversation.open":
                if (!ConversationStore.openConversation(intent.conversationId || "", intent.branchId || ""))
                    return reject("ERR_UI_INVALID_SCOPE", qsTr("This conversation could not be opened."), intent)
                break
            case "conversation.refresh":
                ConversationStore.hydrate()
                break
            case "chat.sendMessage": {
                var text = typeof intent.text === "string" ? intent.text.trim() : ""
                if (text.length === 0)
                    return reject("ERR_UI_EMPTY_MESSAGE", qsTr("Write a message before sending."), intent)
                if (!capabilityStoreRef.canSendMessage || chatStoreRef.streaming)
                    return reject("ERR_UI_ACTION_UNAVAILABLE", qsTr("Mukei is not ready to accept another message yet."), intent)
                var chatScope = ({})
                if (chatStoreRef.conversationId && chatStoreRef.branchId) {
                    chatScope.conversation_id = chatStoreRef.conversationId
                    chatScope.branch_id = chatStoreRef.branchId
                }
                var sendResult = submitBackendCommand("chat.send_message", ({ text: text }), chatScope)
                if (!sendResult.accepted)
                    return false
                if (!chatStoreRef.conversationId && !chatStoreRef.branchId)
                    chatStoreRef.beginPendingScopeAdoption(sendResult.command, false)
                chatStoreRef.setActiveOperationFromAcknowledgement(
                            sendResult.acknowledgement.operation_id || "",
                            sendResult.command)
                chatStoreRef.stageOutgoing(text)
                break
            }
            case "chat.stopGeneration": {
                if (agentSource === null || !capabilityStoreRef.canStopGeneration)
                    return reject("ERR_UI_ACTION_UNAVAILABLE", qsTr("There is no active response to stop."), intent)
                if (!contractStoreRef.scopedCancellationAvailable
                        || !chatStoreRef.activeOperationId
                        || !chatStoreRef.conversationId
                        || !chatStoreRef.branchId)
                    return reject("ERR_UI_STALE_SCOPE", qsTr("The active response can no longer be cancelled safely."), intent)
                var cancelScope = ({
                    conversation_id: chatStoreRef.conversationId,
                    branch_id: chatStoreRef.branchId
                })
                if (chatStoreRef.activeTurnId)
                    cancelScope.turn_id = chatStoreRef.activeTurnId
                var cancelResult = submitBackendCommand(
                            "chat.stop_generation",
                            ({}),
                            cancelScope,
                            ({
                                operationId: chatStoreRef.activeOperationId,
                                idempotencyKey: "cancel:" + chatStoreRef.activeOperationId
                            }))
                if (!cancelResult.accepted)
                    return false
                if (typeof operationStoreRef.markCancellationRequested === "function")
                    operationStoreRef.markCancellationRequested(chatStoreRef.activeOperationId)
                break
            }
            case "chat.updateDraft":
                chatStoreRef.setDraft(
                            typeof intent.text === "string" ? intent.text : "",
                            typeof intent.cursorPosition === "number" ? intent.cursorPosition : 0)
                break
            case "chat.loadOlder":
                chatStoreRef.loadOlderMessages()
                break
            case "chat.clearConversation":
                if (agentSource === null || !capabilityStoreRef.canClearConversation)
                    return reject("ERR_UI_ACTION_UNAVAILABLE", qsTr("This conversation cannot be cleared right now."), intent)
                var clearScope = ({})
                if (chatStoreRef.conversationId && chatStoreRef.branchId) {
                    clearScope.conversation_id = chatStoreRef.conversationId
                    clearScope.branch_id = chatStoreRef.branchId
                }
                if (!submitBackendCommand("chat.clear_conversation", ({}), clearScope).accepted)
                    return false
                break
            case "models.refresh":
                ModelStore.hydrate()
                StorageStore.hydrate()
                break
            case "model.select": {
                var selectModelId = intent.modelId || ""
                var selectIndex = ModelStore.findIndex(selectModelId)
                if (selectIndex < 0 || ModelStore.models.get(selectIndex).installed !== true)
                    return reject("ERR_UI_MODEL_UNAVAILABLE", qsTr("That installed model could not be selected."), intent)
                var selectSubmission = submitBackendCommand(
                            "model.select", ({ model_id: selectModelId }), ({ model_id: selectModelId }))
                if (!selectSubmission.accepted)
                    return false
                ModelStore.beginActivation(selectModelId, "",
                                           selectSubmission.acknowledgement.operation_id || "")
                break
            }
            case "model.delete": {
                var deleteModelId = intent.modelId || ""
                if (!capabilityStoreRef.canDeleteModel)
                    return reject("ERR_UI_ACTION_UNAVAILABLE", qsTr("Models cannot be deleted while Mukei is busy."), intent)
                if (!submitBackendCommand("model.delete", ({ model_id: deleteModelId }), ({ model_id: deleteModelId })).accepted)
                    return false
                break
            }
            case "model.download": {
                var modelId = intent.modelId || ""
                var modelIndex = ModelStore.findIndex(modelId)
                if (modelIndex < 0)
                    return reject("ERR_UI_MODEL_UNKNOWN", qsTr("That model is not available."), intent)
                if (!capabilityStoreRef.canDownloadModel || StorageStore.critical)
                    return reject("ERR_UI_ACTION_UNAVAILABLE", qsTr("A model download cannot start right now."), intent)
                var downloadResult = submitBackendCommand("model.download", ({ model_id: modelId, sha256: "" }), ({ model_id: modelId }))
                if (!downloadResult.accepted)
                    return false
                ModelStore.selectModel(modelId)
                break
            }
            case "download.cancel":
                if (agentSource === null || !capabilityStoreRef.canStopDownload)
                    return reject("ERR_UI_ACTION_UNAVAILABLE", qsTr("There is no active download to stop."), intent)
                if (!submitBackendCommand("download.cancel", ({}), ({})).accepted)
                    return false
                break
            case "downloads.refresh":
                DownloadStore.hydrate()
                StorageStore.hydrate()
                break
            case "documents.refresh":
                DocumentStore.hydrate()
                break
            case "documents.grant":
                if (!submitBackendCommand("document.grant", ({
                    target: intent.target || "",
                    label: intent.label || "",
                    mime_type: intent.mimeType || "application/octet-stream"
                }), ({})).accepted)
                    return false
                break
            case "documents.revoke": {
                var revokeDocumentId = intent.documentId || ""
                if (!revokeDocumentId)
                    return reject("ERR_UI_DOCUMENT_ID", qsTr("That private document could not be identified."), intent)
                if (!submitBackendCommand("document.revoke", ({ document_id: revokeDocumentId }), ({ document_id: revokeDocumentId })).accepted)
                    return false
                break
            }
            case "documents.retryIngestion": {
                var retryDocumentId = intent.documentId || ""
                if (!retryDocumentId)
                    return reject("ERR_UI_DOCUMENT_ID", qsTr("That private document could not be identified."), intent)
                if (!submitBackendCommand("document.retry_ingestion", ({ document_id: retryDocumentId }), ({ document_id: retryDocumentId })).accepted)
                    return false
                break
            }
            case "diagnostics.refresh":
                DiagnosticsStore.hydrate()
                break
            case "diagnostics.export":
                if (!DiagnosticsStore.exportBundle())
                    return false
                break
            case "settings.update":
                if (!intent.key)
                    return reject("ERR_UI_SETTING_KEY", qsTr("That setting could not be updated."), intent)
                if (!submitBackendCommand("settings.update", ({ key: intent.key, value: intent.value }), ({})).accepted)
                    return false
                break
            case "storage.refresh":
                StorageStore.hydrate()
                break
            case "recovery.resume": {
                if (agentSource === null || !RecoveryStore.available)
                    return reject("ERR_UI_RECOVERY_UNAVAILABLE", qsTr("There is no interrupted response to resume."), intent)
                var resumeScope = ({
                    conversation_id: RecoveryStore.conversationId,
                    branch_id: RecoveryStore.branchId
                })
                if (!submitBackendCommand("recovery.resume", ({}), resumeScope).accepted)
                    return false
                break
            }
            case "recovery.regenerate": {
                if (agentSource === null || !RecoveryStore.available)
                    return reject("ERR_UI_RECOVERY_UNAVAILABLE", qsTr("There is no interrupted response to regenerate."), intent)
                var regenerateScope = ({
                    conversation_id: RecoveryStore.conversationId,
                    branch_id: RecoveryStore.branchId
                })
                if (!submitBackendCommand("recovery.regenerate", ({}), regenerateScope).accepted)
                    return false
                break
            }
            case "recovery.dismiss":
                RecoveryStore.clearLocal()
                NavigationStore.navigate("chat", ({}), true)
                break
            default:
                return reject("ERR_UI_UNKNOWN_INTENT", qsTr("This action is not supported yet."), intent)
            }
        } catch (error) {
            return reject("ERR_UI_INTENT_FAILED", qsTr("Mukei could not complete that action."), intent)
        }

        intentAccepted(intent.type)
        return true
    }
}
