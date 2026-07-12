pragma Singleton
import QtQuick

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

    Connections {
        target: UiSessionStore
        function onHydrationCompleted() { AppCoordinator.tryFinishReadyHydration() }
    }

    Connections {
        target: RecoveryStore
        function onHydrationCompleted() { AppCoordinator.tryFinishReadyHydration() }
    }

    Connections {
        target: EventDispatcher
        function onEventReceived(event) {
            AppCoordinator.applyEvent(event)
        }
        function onStreamSequenceGapDetected(feature, streamId, expectedSequence, receivedSequence) {
            SnapshotController.requestFeatureSnapshot(feature, expectedSequence, receivedSequence)
            EventDispatcher.completeResynchronization(streamId, receivedSequence)
            SnapshotController.markApplied(feature)
        }
    }

    Connections {
        target: SnapshotController
        function onSnapshotRequested(feature) {
            if (feature === "chat")
                ChatStore.requestSnapshot(false)
            else if (feature === "downloads")
                DownloadStore.hydrate()
            else if (feature === "models")
                ModelStore.hydrate()
            else if (feature === "documents")
                DocumentStore.hydrate()
            else if (feature === "storage")
                StorageStore.hydrate()
            else if (feature === "operations" || feature === "errors")
                OperationStore.hydrate()
            else if (feature === "app") {
                ConversationStore.hydrate()
                RecoveryStore.hydrate()
                ModelStore.hydrate()
                DownloadStore.hydrate()
                DocumentStore.hydrate()
                StorageStore.hydrate()
                SettingsStore.hydrate()
                DiagnosticsStore.hydrate()
            }
        }
    }
}
