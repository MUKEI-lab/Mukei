pragma Singleton
import QtQuick
import "../events"
import "../stores"

Item {
    property bool configured: false
    property bool started: false
    property bool readyHydrated: false
    property bool readyHydrationPending: false
    property var runtimeSource: null

    signal architectureReady
    signal readyStateHydrated

    function configure(agent, bridge, runtime) {
        ContractStore.configure(agent)
        EventDispatcher.agentSource = agent
        EventDispatcher.bridgeSource = bridge
        IntentDispatcher.configure(agent, bridge, runtime)
        UiSessionStore.configure(agent)
        ConversationStore.configure(agent)
        ChatStore.configure(agent)
        RecoveryStore.configure(agent)
        ModelStore.configure(bridge, agent)
        DownloadStore.configure(agent)
        DocumentStore.configure(agent)
        StorageStore.configure(agent)
        SettingsStore.configure(agent)
        DiagnosticsStore.configure(agent)
        OperationStore.configure(agent)
        runtimeSource = runtime
        configured = true
    }

    function continueStartupAfterContract() {
        LifecycleStore.setLocalState("bootstrapping", "")
        NavigationStore.syncWithLifecycle(LifecycleStore.state)
        if (runtimeSource && runtimeSource.autoInitialize === true) {
            IntentDispatcher.dispatch({
                type: "app.initialize",
                configPath: runtimeSource.configPath
            })
        } else {
            LifecycleStore.setLocalState("needs_database_key", "")
            NavigationStore.syncWithLifecycle(LifecycleStore.state)
        }
    }

    function start() {
        if (!configured || started)
            return
        started = true
        if (!ContractStore.hydrate()) {
            LifecycleStore.setLocalState("incompatible_contract", ContractStore.safeMessage)
            NavigationStore.syncWithLifecycle(LifecycleStore.state)
            architectureReady()
            return
        }
        continueStartupAfterContract()
        architectureReady()
    }

    function retryContractNegotiation() {
        if (!configured)
            return false
        if (!ContractStore.hydrate()) {
            LifecycleStore.setLocalState("incompatible_contract", ContractStore.safeMessage)
            NavigationStore.syncWithLifecycle(LifecycleStore.state)
            return false
        }
        readyHydrated = false
        readyHydrationPending = false
        continueStartupAfterContract()
        return true
    }

    function onApplicationStateChanged(applicationState) {
        if (applicationState !== Qt.ApplicationActive) {
            UiSessionStore.flushNow()
            return
        }
        if (!ContractStore.compatible) {
            retryContractNegotiation()
            return
        }
        if (LifecycleStore.ready) {
            ConversationStore.hydrate()
            DownloadStore.hydrate()
            DocumentStore.hydrate()
            StorageStore.hydrate()
            DiagnosticsStore.hydrate()
            OperationStore.hydrate()
            if (ChatStore.conversationId && ChatStore.branchId)
                ChatStore.requestSnapshot(false)
        }
    }

    function hydrateReadyState() {
        if (readyHydrated || readyHydrationPending)
            return
        readyHydrationPending = true
        UiSessionStore.hydrate()
        ConversationStore.hydrate()
        RecoveryStore.hydrate()
        ModelStore.hydrate()
        DownloadStore.hydrate()
        DocumentStore.hydrate()
        StorageStore.hydrate()
        SettingsStore.hydrate()
        DiagnosticsStore.hydrate()
        OperationStore.hydrate()
        tryFinishReadyHydration()
    }

    function tryFinishReadyHydration() {
        if (!readyHydrationPending || !UiSessionStore.hydrated || !RecoveryStore.hydrated)
            return

        if (RecoveryStore.available) {
            NavigationStore.navigate("recovery", ({
                conversationId: RecoveryStore.conversationId,
                branchId: RecoveryStore.branchId
            }), true)
        } else {
            if (UiSessionStore.activeConversationId && UiSessionStore.activeBranchId)
                ChatStore.openConversation(
                            UiSessionStore.activeConversationId,
                            UiSessionStore.activeBranchId)
            else
                ChatStore.restoreLastConversation()

            var safeRoute = ["chat", "models", "downloads", "documents", "settings"].indexOf(UiSessionStore.activeRoute) >= 0
                    ? UiSessionStore.activeRoute : "chat"
            NavigationStore.navigate(safeRoute,
                                     safeRoute === "chat" ? ({
                                         conversationId: UiSessionStore.activeConversationId,
                                         branchId: UiSessionStore.activeBranchId
                                     }) : UiSessionStore.activeRouteParameters,
                                     true)
        }
        readyHydrationPending = false
        readyHydrated = true
        readyStateHydrated()
    }

    function applyEvent(event) {
        CapabilityStore.applyEvent(event)
        LifecycleStore.applyEvent(event)
        OperationStore.applyEvent(event)
        ChatStore.applyEvent(event)
        DownloadStore.applyEvent(event)
        ModelStore.applyEvent(event)
        ErrorStore.applyEvent(event)

        if ((event.command_type === "recovery.resume" || event.command_type === "recovery.regenerate")
                && event.category === "chat_state" && event.state === "submitting"
                && event.operation_id) {
            var conversationId = event.conversation_id || RecoveryStore.conversationId
            var branchId = event.branch_id || RecoveryStore.branchId
            if (RecoveryStore.markClaimed(event.operation_id, conversationId, branchId)) {
                if (conversationId && branchId)
                    ChatStore.openConversation(conversationId, branchId)
                NavigationStore.navigate("chat", ({
                    conversationId: conversationId,
                    branchId: branchId
                }), true)
            }
        }

        if (event.category === "operation_lifecycle" && event.state === "completed") {
            switch (event.command_type || "") {
            case "chat.clear_conversation":
                ChatStore.reset()
                ConversationStore.hydrate()
                break
            case "model.select":
                ModelStore.hydrate()
                break
            case "model.delete":
                ModelStore.hydrate()
                StorageStore.hydrate()
                break
            case "document.grant":
            case "document.revoke":
            case "document.retry_ingestion":
                DocumentStore.hydrate()
                OperationStore.reconcileDurableState()
                break
            case "settings.update":
                SettingsStore.hydrate()
                break
            default:
                break
            }
        }

        if (event.category === "app_lifecycle") {
            NavigationStore.syncWithLifecycle(event.state)
            if (event.state === "ready" || event.state === "degraded")
                Qt.callLater(hydrateReadyState)
            else if (["quarantined", "audit_quarantined", "key_invalidated",
                      "wrapped_key_corrupt", "database_open_failed", "reset_required"].indexOf(event.state) >= 0) {
                readyHydrated = false
                readyHydrationPending = false
            }
        }
    }

    function scheduleResyncRetry(streamId, resyncId) {
        Qt.callLater(function() {
            SnapshotController.retry(streamId, resyncId)
        })
    }

    function retryWaitingFeatureSnapshots(feature) {
        var waiting = SnapshotController.waitingTicketsForFeature(feature)
        for (var i = 0; i < waiting.length; ++i)
            scheduleResyncRetry(waiting[i].streamId, waiting[i].resyncId)
    }

    function finishFeatureResynchronization(feature) {
        var tickets = SnapshotController.inFlightTicketsForFeature(feature)
        for (var i = 0; i < tickets.length; ++i) {
            var ticket = tickets[i]
            var watermark = Number(ticket.requestWatermark)
            if (!SnapshotController.validateApplied(ticket.streamId, ticket.resyncId, watermark)) {
                scheduleResyncRetry(ticket.streamId, ticket.resyncId)
                continue
            }
            if (!EventDispatcher.completeResynchronization(ticket.streamId, watermark)) {
                SnapshotController.markFailed(ticket.streamId, ticket.resyncId,
                                              "dispatcher_rejected_snapshot_watermark")
                continue
            }
            SnapshotController.markApplied(ticket.streamId, ticket.resyncId)
        }
        retryWaitingFeatureSnapshots(feature)
    }

    function failFeatureResynchronization(feature, reason) {
        var tickets = SnapshotController.inFlightTicketsForFeature(feature)
        for (var i = 0; i < tickets.length; ++i)
            SnapshotController.markFailed(tickets[i].streamId, tickets[i].resyncId, reason)
        retryWaitingFeatureSnapshots(feature)
    }

    function startFeatureSnapshot(feature, streamId, resyncId, expectedSequence, snapshotWatermark) {
        if (feature === "chat") {
            if (!SnapshotController.markRequestStarted(streamId, resyncId, snapshotWatermark))
                return
            if (!ChatStore.requestSnapshot(false))
                SnapshotController.markFailed(streamId, resyncId, "chat_snapshot_request_failed")
            return
        }

        if (feature === "downloads") {
            if (DownloadStore.loading) {
                SnapshotController.markWaiting(streamId, resyncId)
                return
            }
            if (!SnapshotController.markRequestStarted(streamId, resyncId, snapshotWatermark))
                return
            DownloadStore.hydrate()
            return
        }

        // No authoritative, completion-correlated snapshot contract exists for
        // these streams yet. Keep them quarantined rather than reopening the
        // event gate on a best-effort hydrate.
        SnapshotController.markFailed(streamId, resyncId,
                                      "authoritative_snapshot_not_supported_for_" + feature)
    }

    Connections {
        target: UiSessionStore
        function onHydrationCompleted() { AppCoordinator.tryFinishReadyHydration() }
    }

    Connections {
        target: RecoveryStore
        function onHydrationCompleted() { AppCoordinator.tryFinishReadyHydration() }
    }

    Connections {
        target: ChatStore
        function onSnapshotApplied() {
            AppCoordinator.finishFeatureResynchronization("chat")
        }
    }

    Connections {
        target: DownloadStore
        function onSnapshotApplied() {
            AppCoordinator.finishFeatureResynchronization("downloads")
        }
        function onSnapshotFailed() {
            AppCoordinator.failFeatureResynchronization("downloads", "download_snapshot_failed")
        }
    }

    Connections {
        target: EventDispatcher
        function onEventReceived(event) {
            AppCoordinator.applyEvent(event)
        }
        function onStreamSequenceGapDetected(feature, streamId, expectedSequence, receivedSequence) {
            SnapshotController.requestFeatureSnapshot(feature, streamId,
                                                      expectedSequence, receivedSequence)
        }
        function onStreamQuarantineAdvanced(streamId, observedSequence) {
            SnapshotController.noteObservedSequence(streamId, observedSequence)
        }
    }

    Connections {
        target: SnapshotController
        function onSnapshotRequested(feature, streamId, resyncId, expectedSequence, snapshotWatermark) {
            AppCoordinator.startFeatureSnapshot(feature, streamId, resyncId,
                                                expectedSequence, snapshotWatermark)
        }
    }
}
