pragma Singleton
import QtQuick
import QtQml.Models

Item {
    id: root

    property var agentSource: null
    property var fallbackTimeline: ListModel { id: fallbackTimelineModel; dynamicRoles: true }
    property var timeline: typeof mukeiTimelineModel !== "undefined"
                           ? mukeiTimelineModel : fallbackTimelineModel
    property string activeConversationId: ""
    property string activeBranchId: ""
    property string turnState: "idle"
    property bool streaming: false
    property string activeAssistantRowId: ""
    property string pendingStreamText: ""
    property int streamBatchIntervalMs: 48
    property int rowCounter: 0
    property string draft: ""
    property int draftCursorPosition: 0
    property bool snapshotLoading: false
    property bool olderPageLoading: false
    property var dirtyBackgroundScopes: ({})
    property string activeOperationId: ""
    property string activeTurnId: ""
    readonly property string conversationId: activeConversationId
    readonly property string branchId: activeBranchId

    // Fresh-chat scope adoption is a bounded capability granted only by an
    // explicit user send. Protocol V2 events must correlate to this marker;
    // the marker is cleared on adoption, rejection, terminal failure, expiry,
    // explicit navigation, or supersession.
    property var pendingScopeAdoption: ({})
    property int pendingScopeAdoptionGeneration: 0
    property int pendingScopeAdoptionTtlMs: 30 * 1000
    property bool awaitingInitialScopeBinding: false
    property bool hasOlderMessages: timeline && typeof timeline.hasOlder !== "undefined"
                                      ? timeline.hasOlder : false
    readonly property bool hasMessages: timeline && typeof timeline.count === "number"
                                        ? timeline.count > 0 : false

    signal tailUpdated
    signal draftChangedByStore(string draft, int cursorPosition)
    signal snapshotApplied

    Timer {
        id: streamBatchTimer
        interval: root.streamBatchIntervalMs
        repeat: false
        onTriggered: root.flushPendingStreamText()
    }

    Timer {
        id: pendingScopeExpiryTimer
        interval: 1000
        repeat: true
        running: root.awaitingInitialScopeBinding
        onTriggered: root.expirePendingScopeAdoption()
    }

    function configure(agent) {
        agentSource = agent
    }

    function hasPendingScopeAdoption() {
        return awaitingInitialScopeBinding
                && pendingScopeAdoption
                && typeof pendingScopeAdoption === "object"
                && typeof pendingScopeAdoption.expiresAt === "number"
    }

    function clearPendingScopeAdoption(reason) {
        awaitingInitialScopeBinding = false
        pendingScopeAdoption = ({})
    }

    function expirePendingScopeAdoption() {
        if (hasPendingScopeAdoption() && Date.now() >= pendingScopeAdoption.expiresAt)
            clearPendingScopeAdoption("expired")
    }

    function beginPendingScopeAdoption(envelope, legacyMode) {
        if (activeConversationId || activeBranchId)
            return false
        if (!envelope || typeof envelope !== "object")
            return false

        pendingScopeAdoptionGeneration += 1
        pendingScopeAdoption = {
            generation: pendingScopeAdoptionGeneration,
            commandId: envelope.command_id || "",
            requestId: envelope.request_id || "",
            correlationId: envelope.correlation_id || "",
            operationId: envelope.operation_id || "",
            protocolMode: legacyMode === true ? "legacy_v1" : "v2",
            createdAt: Date.now(),
            expiresAt: Date.now() + (legacyMode === true ? 5000 : pendingScopeAdoptionTtlMs),
            legacySequenceFloor: typeof EventDispatcher.lastSequence === "number"
                                 ? EventDispatcher.lastSequence : 0
        }
        awaitingInitialScopeBinding = true
        return true
    }

    function updatePendingScopeOperation(commandId, operationId) {
        if (!hasPendingScopeAdoption() || !operationId
                || pendingScopeAdoption.commandId !== commandId)
            return false
        var next = Object.assign({}, pendingScopeAdoption)
        next.operationId = operationId
        pendingScopeAdoption = next
        return true
    }

    function clearPendingScopeAdoptionForCommand(commandId) {
        if (!hasPendingScopeAdoption() || pendingScopeAdoption.commandId !== commandId)
            return false
        clearPendingScopeAdoption("command_resolved_without_adoption")
        return true
    }

    function pendingScopeMarkerMatchesEvent(event) {
        expirePendingScopeAdoption()
        if (!hasPendingScopeAdoption() || !event)
            return false

        var marker = pendingScopeAdoption
        if (marker.protocolMode === "v2") {
            if (event.protocol_mode !== "v2")
                return false
            var matched = false
            if (marker.operationId) {
                if (event.operation_id !== marker.operationId)
                    return false
                matched = true
            }
            if (event.command_id) {
                if (event.command_id !== marker.commandId)
                    return false
                matched = true
            }
            if (event.request_id) {
                if (event.request_id !== marker.requestId)
                    return false
                matched = true
            }
            if (event.correlation_id) {
                if (event.correlation_id !== marker.correlationId)
                    return false
                matched = true
            }
            return matched
        }

        // Explicit legacy compatibility: V1 has no authoritative operation or
        // request correlation. Keep the old bootstrap behavior narrowly bounded
        // to the first submitting transition immediately after the explicit send
        // and never claim this as Protocol V2 scope safety.
        return event.protocol_mode === "legacy_v1"
                && event.category === "chat_state"
                && event.state === "submitting"
                && typeof event.sequence === "number"
                && event.sequence > marker.legacySequenceFloor
                && Date.now() < marker.expiresAt
    }

    function setActiveOperationFromAcknowledgement(operationId, envelope) {
        if (!operationId)
            return false
        activeOperationId = operationId
        if (envelope && envelope.scope && envelope.scope.turn_id)
            activeTurnId = envelope.scope.turn_id
        if (envelope && envelope.command_id)
            updatePendingScopeOperation(envelope.command_id, operationId)
        return true
    }

    function newRowId(prefix) {
        rowCounter += 1
        return prefix + "-" + Date.now() + "-" + rowCounter
    }

    function scopeKey(conversationId, branchId) {
        var conversation = conversationId || ""
        var branch = branchId || ""
        return conversation.length + ":" + conversation + ":" + branch
    }

    function isActiveScope(conversationId, branchId) {
        return conversationId === activeConversationId && branchId === activeBranchId
    }

    function classifyEventScope(event) {
        if (!event)
            return ({ kind: "unscoped", conversationId: "", branchId: "" })

        var hasConversation = typeof event.conversation_id !== "undefined"
                && event.conversation_id !== null
        var hasBranch = typeof event.branch_id !== "undefined"
                && event.branch_id !== null

        if (!hasConversation && !hasBranch)
            return ({ kind: "unscoped", conversationId: "", branchId: "" })

        if (hasConversation !== hasBranch
                || typeof event.conversation_id !== "string"
                || typeof event.branch_id !== "string"
                || event.conversation_id.length === 0
                || event.branch_id.length === 0) {
            return ({ kind: "malformed", conversationId: "", branchId: "" })
        }

        var conversationId = event.conversation_id
        var branchId = event.branch_id
        return ({
            kind: isActiveScope(conversationId, branchId) ? "active" : "background",
            conversationId: conversationId,
            branchId: branchId
        })
    }

    function markBackgroundScopeDirty(conversationId, branchId, category) {
        if (!conversationId || !branchId)
            return
        var key = scopeKey(conversationId, branchId)
        var next = Object.assign({}, dirtyBackgroundScopes)
        next[key] = {
            conversationId: conversationId,
            branchId: branchId,
            lastCategory: category || "",
            lastActivityAt: Date.now(),
            needsRefresh: true
        }
        dirtyBackgroundScopes = next
    }

    function clearBackgroundScopeDirty(conversationId, branchId) {
        var key = scopeKey(conversationId, branchId)
        if (typeof dirtyBackgroundScopes[key] === "undefined")
            return
        var next = Object.assign({}, dirtyBackgroundScopes)
        delete next[key]
        dirtyBackgroundScopes = next
    }

    function isChatEvent(event) {
        return event && [
            "chat_state",
            "chat_chunk",
            "chat_completed",
            "chat_cancelled",
            "chat_failed"
        ].indexOf(event.category) >= 0
    }

    function discardVisibleTurnProjection() {
        streamBatchTimer.stop()
        pendingStreamText = ""
        activeAssistantRowId = ""
        turnState = "idle"
        streaming = false
        AccessibilityStore.reset()
    }

    function modelCount() {
        return timeline && typeof timeline.count === "number" ? timeline.count : 0
    }

    function clearTimeline() {
        if (timeline && typeof timeline.clear === "function")
            timeline.clear()
    }

    function appendRow(row) {
        if (timeline && typeof timeline.appendRow === "function")
            timeline.appendRow(row)
        else if (timeline && typeof timeline.append === "function")
            timeline.append(row)
    }

    function appendText(rowId, chunk) {
        if (timeline && typeof timeline.appendText === "function")
            return timeline.appendText(rowId, chunk)
        for (var i = 0; i < timeline.count; ++i) {
            if (timeline.get(i).rowId === rowId) {
                timeline.setProperty(i, "text", (timeline.get(i).text || "") + chunk)
                timeline.setProperty(i, "status", "streaming")
                return true
            }
        }
        return false
    }

    function updateStatus(rowId, status) {
        if (!rowId)
            return false
        if (timeline && typeof timeline.updateStatus === "function")
            return timeline.updateStatus(rowId, status)
        for (var i = 0; i < timeline.count; ++i) {
            if (timeline.get(i).rowId === rowId) {
                timeline.setProperty(i, "status", status)
                return true
            }
        }
        return false
    }

    function reset() {
        AccessibilityStore.reset()
        clearPendingScopeAdoption("reset")
        clearTimeline()
        activeAssistantRowId = ""
        pendingStreamText = ""
        streamBatchTimer.stop()
        turnState = "idle"
        streaming = false
        activeOperationId = ""
        activeTurnId = ""
        activeConversationId = ""
        activeBranchId = ""
        UiSessionStore.setActiveChatScope("", "")
    }

    function setDraft(text, cursorPosition) {
        var nextText = text || ""
        var nextCursor = Math.max(0, cursorPosition || 0)
        if (draft === nextText && draftCursorPosition === nextCursor)
            return
        draft = nextText
        draftCursorPosition = nextCursor
        UiSessionStore.saveDraft(activeConversationId, activeBranchId, draft, draftCursorPosition)
    }

    function restoreDraft() {
        draft = UiSessionStore.loadDraft(activeConversationId, activeBranchId)
        draftCursorPosition = UiSessionStore.cursorPosition(activeConversationId, activeBranchId)
        draftChangedByStore(draft, draftCursorPosition)
    }

    function openConversation(conversationId, branchId) {
        if (!conversationId || !branchId)
            return false
        // Explicit navigation is the only ordinary owner of visible scope and
        // invalidates any pending first-send adoption grant.
        clearPendingScopeAdoption("explicit_navigation")
        var scopeChanged = !isActiveScope(conversationId, branchId)
        if (scopeChanged) {
            discardVisibleTurnProjection()
            activeOperationId = ""
            activeTurnId = ""
            clearTimeline()
        }
        activeConversationId = conversationId
        activeBranchId = branchId
        UiSessionStore.setActiveChatScope(conversationId, branchId)
        restoreDraft()
        return requestSnapshot(false)
    }

    function restoreLastConversation() {
        if (UiSessionStore.activeConversationId && UiSessionStore.activeBranchId)
            return openConversation(UiSessionStore.activeConversationId, UiSessionStore.activeBranchId)
        clearTimeline()
        restoreDraft()
        return false
    }

    function requestSnapshot(prepend) {
        if (agentSource === null || typeof agentSource.chat_snapshot_json !== "function")
            return false
        if (!activeConversationId || !activeBranchId)
            return false
        var beforeId = prepend && timeline && timeline.oldestMessageId
                ? timeline.oldestMessageId : ""
        if (prepend)
            olderPageLoading = true
        else
            snapshotLoading = true
        try {
            var json = agentSource.chat_snapshot_json(
                        activeConversationId,
                        activeBranchId,
                        beforeId,
                        80)
            var parsed = JSON.parse(json)
            if (parsed && parsed.error) {
                ErrorStore.push(parsed.error, "ERR_UI_CHAT_SNAPSHOT")
                return false
            }
            var applied = false
            if (timeline && prepend && typeof timeline.prependFromSnapshotJson === "function")
                applied = timeline.prependFromSnapshotJson(json)
            else if (timeline && !prepend && typeof timeline.replaceFromSnapshotJson === "function")
                applied = timeline.replaceFromSnapshotJson(json)
            else if (timeline && typeof timeline.clear === "function") {
                if (!prepend)
                    timeline.clear()
                var items = parsed.items || []
                for (var i = 0; i < items.length; ++i)
                    prepend ? timeline.insert(i, items[i]) : timeline.append(items[i])
                applied = true
            }
            if (applied) {
                SnapshotController.markApplied("chat")
                UiSessionStore.setTimelineAnchor(parsed.oldest_message_id || "")
                if (!prepend) {
                    clearBackgroundScopeDirty(activeConversationId, activeBranchId)
                    // Snapshot reconciliation is the explicit resynchronization
                    // boundary for a quarantined chat stream. If the snapshot
                    // exposes a sequence use it; otherwise EventDispatcher uses
                    // the gap high-water mark that triggered this refresh.
                    EventDispatcher.markChatScopeResynchronized(
                                activeConversationId,
                                activeBranchId,
                                typeof parsed.stream_sequence === "number"
                                ? parsed.stream_sequence : undefined)
                }
                snapshotApplied()
                tailUpdated()
            }
            return applied
        } catch (error) {
            ErrorStore.push({
                code: "ERR_UI_CHAT_SNAPSHOT",
                severity: "warning",
                recoverable: true,
                safe_message: qsTr("This conversation could not be restored.")
            })
            return false
        } finally {
            snapshotLoading = false
            olderPageLoading = false
        }
    }

    function loadOlderMessages() {
        if (olderPageLoading || !hasOlderMessages)
            return false
        return requestSnapshot(true)
    }

    function stageOutgoing(text) {
        var clean = (text || "").trim()
        if (clean.length === 0)
            return false
        var rowId = newRowId("user")
        appendRow({
            rowId: rowId,
            type: "user_message",
            text: clean,
            phase: "",
            kind: "",
            status: "pending",
            timestamp: qsTr("Now"),
            toolName: "",
            parentId: "",
            conversationId: activeConversationId,
            branchId: activeBranchId
        })
        UiSessionStore.clearDraft(activeConversationId, activeBranchId)
        draft = ""
        draftCursorPosition = 0
        draftChangedByStore("", 0)
        tailUpdated()
        return true
    }

    function ensureAssistantRow() {
        if (activeAssistantRowId)
            return activeAssistantRowId
        activeAssistantRowId = newRowId("assistant")
        appendRow({
            rowId: activeAssistantRowId,
            type: "assistant_message",
            text: "",
            phase: "",
            kind: "",
            status: "streaming",
            timestamp: qsTr("Now"),
            toolName: "",
            parentId: "",
            conversationId: activeConversationId,
            branchId: activeBranchId
        })
        tailUpdated()
        return activeAssistantRowId
    }

    function flushPendingStreamText() {
        if (!pendingStreamText)
            return
        var chunk = pendingStreamText
        pendingStreamText = ""
        appendText(ensureAssistantRow(), chunk)
        tailUpdated()
    }

    function appendChunk(chunk) {
        if (!chunk)
            return
        ensureAssistantRow()
        pendingStreamText += chunk
        AccessibilityStore.enqueueChunk(chunk)
        if (!streamBatchTimer.running)
            streamBatchTimer.start()
    }

    function finalizeAssistant(status) {
        streamBatchTimer.stop()
        flushPendingStreamText()
        AccessibilityStore.flush()
        updateStatus(activeAssistantRowId, status)
        activeAssistantRowId = ""
        tailUpdated()
        if (activeConversationId && activeBranchId)
            Qt.callLater(function() { root.requestSnapshot(false) })
    }

    function appendSystemEvent(text, phase, kind, toolName) {
        appendRow({
            rowId: newRowId("event"),
            type: "timeline_event",
            text: text,
            phase: phase || "result",
            kind: kind || "system",
            status: phase || "result",
            timestamp: qsTr("Now"),
            toolName: toolName || "",
            parentId: "",
            conversationId: activeConversationId,
            branchId: activeBranchId
        })
        tailUpdated()
    }

    function applyEvent(event) {
        if (!isChatEvent(event))
            return

        var scope = classifyEventScope(event)
        if (scope.kind === "malformed")
            return

        // Fresh-chat exception: adoption requires the bounded marker created by
        // explicit send intent. V2 must correlate by operation/request/command;
        // unrelated delayed background events cannot consume the grant.
        if (scope.kind === "background"
                && !activeConversationId
                && !activeBranchId
                && pendingScopeMarkerMatchesEvent(event)) {
            activeConversationId = scope.conversationId
            activeBranchId = scope.branchId
            activeOperationId = event.operation_id || pendingScopeAdoption.operationId || ""
            activeTurnId = event.turn_id || ""
            UiSessionStore.setActiveChatScope(activeConversationId, activeBranchId)
            clearPendingScopeAdoption("scope_adopted")
            scope = ({
                kind: "active",
                conversationId: activeConversationId,
                branchId: activeBranchId
            })
        }

        if (scope.kind === "unscoped") {
            // Missing chat scope is deliberate failure, never an implicit alias
            // for whichever conversation happens to be visible.
            return
        }

        if (scope.kind === "background") {
            markBackgroundScopeDirty(scope.conversationId, scope.branchId, event.category)
            return
        }

        // Within the same chat scope, a delayed event from an older V2
        // operation is still background history and must not overwrite the
        // visible stream projection.
        if (event.protocol_mode === "v2") {
            if (!activeOperationId || event.operation_id !== activeOperationId) {
                markBackgroundScopeDirty(scope.conversationId, scope.branchId, event.category)
                return
            }
            if (activeTurnId && event.turn_id && event.turn_id !== activeTurnId) {
                markBackgroundScopeDirty(scope.conversationId, scope.branchId, event.category)
                return
            }
            if (!activeTurnId && event.turn_id)
                activeTurnId = event.turn_id
        }

        switch (event.category) {
        case "chat_state":
            turnState = event.state
            streaming = ["submitting", "thinking", "streaming", "tool_calling", "cancelling"].indexOf(event.state) >= 0
            if (event.state === "thinking")
                ensureAssistantRow()
            if (["failed", "cancelled", "completed"].indexOf(event.state) >= 0) {
                if (event.protocol_mode === "v2") {
                    // Protocol V2 permits a terminal chat_state to be the sole
                    // terminal event. Finalize visible projection and release
                    // operation identity here rather than waiting for a second
                    // terminal event that reliability guards would reject.
                    if (event.state === "completed") {
                        AccessibilityStore.announceStatus(qsTr("Mukei finished responding."))
                        finalizeAssistant("completed")
                        ConversationStore.hydrate()
                        RecoveryStore.hydrate()
                    } else if (event.state === "cancelled") {
                        AccessibilityStore.announceStatus(qsTr("Response stopped."))
                        finalizeAssistant("cancelled")
                        RecoveryStore.hydrate()
                    } else {
                        AccessibilityStore.announceStatus(qsTr("The response could not be completed."))
                        finalizeAssistant("failed")
                        appendSystemEvent(event.error && (event.error.user_message || event.error.safe_message)
                                          ? (event.error.user_message || event.error.safe_message)
                                          : qsTr("The response could not be completed."), "failure", "system", "")
                        RecoveryStore.hydrate()
                    }
                    activeOperationId = ""
                    activeTurnId = ""
                } else if (activeConversationId && activeBranchId) {
                    Qt.callLater(function() { root.requestSnapshot(false) })
                }
            }
            break
        case "chat_chunk":
            streaming = true
            turnState = "streaming"
            appendChunk(event.chunk)
            break
        case "chat_completed":
            AccessibilityStore.announceStatus(qsTr("Mukei finished responding."))
            streaming = false
            turnState = "completed"
            finalizeAssistant("completed")
            activeOperationId = ""
            activeTurnId = ""
            ConversationStore.hydrate()
            RecoveryStore.hydrate()
            break
        case "chat_cancelled":
            AccessibilityStore.announceStatus(qsTr("Response stopped."))
            streaming = false
            turnState = "cancelled"
            finalizeAssistant("cancelled")
            activeOperationId = ""
            activeTurnId = ""
            RecoveryStore.hydrate()
            break
        case "chat_failed":
            AccessibilityStore.announceStatus(qsTr("The response could not be completed."))
            streaming = false
            turnState = "failed"
            finalizeAssistant("failed")
            activeOperationId = ""
            activeTurnId = ""
            if (!activeConversationId || !activeBranchId)
                clearPendingScopeAdoption("terminal_failure_without_scope")
            appendSystemEvent(event.error && (event.error.user_message || event.error.safe_message)
                              ? (event.error.user_message || event.error.safe_message)
                              : qsTr("The response could not be completed."), "failure", "system", "")
            RecoveryStore.hydrate()
            break
        }
    }
}
