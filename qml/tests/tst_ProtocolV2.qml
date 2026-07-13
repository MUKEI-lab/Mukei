import QtQuick
import QtTest
import "../architecture"
import "../events"
import "../stores"

TestCase {
    name: "ProtocolV2MergedSemantics"

    QtObject {
        id: protocolAgent
        property string acknowledgementMode: "accepted"
        property string lastCommandJson: ""

        function ui_contract_snapshot_json() {
            return JSON.stringify({
                schema_version: 1,
                contract_version: 1,
                min_qml_contract_version: 1,
                max_qml_contract_version: 1,
                command_schema_version: 2,
                event_schema_version: 2,
                snapshot_schema_version: 1,
                required_features: [
                    "typed_commands", "typed_events", "snapshot_delta_sync",
                    "persistent_ui_session", "capability_gating",
                    "command_envelope_v2", "command_acknowledgement",
                    "event_identity", "per_stream_sequencing",
                    "idempotent_command_replay", "operation_lifecycle_events",
                    "legacy_event_v1_compatibility"
                ],
                protocol: {
                    current_version: { major: 2, minor: 0 },
                    minimum_supported_peer_major: 2,
                    capabilities: [
                        "command_envelope_v2", "command_acknowledgement",
                        "event_identity", "per_stream_sequencing",
                        "idempotent_command_replay", "operation_lifecycle_events",
                        "scoped_chat_operations", "legacy_event_v1_compatibility"
                    ]
                }
            })
        }

        function submit_command_json(commandJson) {
            lastCommandJson = commandJson
            if (acknowledgementMode === "malformed")
                return "{bad-json"
            var command = JSON.parse(commandJson)
            if (acknowledgementMode === "accepted_missing_operation") {
                return JSON.stringify({
                    protocol_version: { major: 2, minor: 0 },
                    status: "accepted",
                    command_id: command.command_id,
                    request_id: command.request_id,
                    correlation_id: command.correlation_id,
                    timestamp: new Date().toISOString()
                })
            }
            if (acknowledgementMode === "rejected") {
                return JSON.stringify({
                    protocol_version: { major: 2, minor: 0 },
                    status: "rejected",
                    command_id: command.command_id,
                    request_id: command.request_id,
                    correlation_id: command.correlation_id,
                    rejection_reason: "busy_conflict",
                    timestamp: new Date().toISOString()
                })
            }
            return JSON.stringify({
                protocol_version: { major: 2, minor: 0 },
                status: "accepted",
                command_id: command.command_id,
                request_id: command.request_id,
                correlation_id: command.correlation_id,
                operation_id: command.operation_id || "operation-accepted",
                timestamp: new Date().toISOString()
            })
        }
    }

    function readyCapabilities() {
        return {
            can_initialize: false,
            can_send_message: true,
            can_stop_generation: true,
            can_download_model: true,
            can_stop_download: false,
            can_switch_model: true,
            can_delete_model: true,
            can_clear_conversation: true,
            can_open_settings: true,
            needs_config: false,
            needs_storage_permission: false,
            active_model_ready: true,
            is_busy: false,
            is_downloading: false,
            is_inferencing: false
        }
    }

    function init() {
        protocolAgent.acknowledgementMode = "accepted"
        protocolAgent.lastCommandJson = ""
        OperationStore.operations.clear()
        ChatStore.reset()
        EventDispatcher.reset()
        ContractStore.configure(protocolAgent)
        verify(ContractStore.hydrate())
        compare(ContractStore.protocolMode, "v2")
        verify(ContractStore.scopedCancellationAvailable)
        IntentDispatcher.configure(protocolAgent, null, null)
        IntentDispatcher.configureProtocolDependencies(
                    ContractStore, CapabilityStore, ChatStore, OperationStore)
        CapabilityStore.applySnapshot(readyCapabilities())
    }

    function cleanup() {
        ChatStore.reset()
        OperationStore.operations.clear()
        ContractStore.reset()
    }

    function test_malformed_ack_fails_closed() {
        protocolAgent.acknowledgementMode = "malformed"
        verify(!IntentDispatcher.dispatch({ type: "chat.sendMessage", text: "hello" }))
        compare(OperationStore.operations.count, 1)
        compare(OperationStore.operations.get(0).state, "rejected")
        verify(!ChatStore.awaitingInitialScopeBinding)
        compare(ChatStore.activeOperationId, "")
    }

    function test_fresh_send_arms_bounded_scope_adoption() {
        verify(IntentDispatcher.dispatch({ type: "chat.sendMessage", text: "hello" }))
        verify(ChatStore.awaitingInitialScopeBinding)
        compare(ChatStore.activeOperationId, "operation-accepted")
        verify(ChatStore.pendingScopeAdoption.commandId.length > 0)
        verify(ChatStore.pendingScopeAdoption.requestId.length > 0)
    }

    function test_correlated_first_event_adopts_authoritative_scope() {
        verify(IntentDispatcher.dispatch({ type: "chat.sendMessage", text: "hello" }))
        var command = JSON.parse(protocolAgent.lastCommandJson)
        ChatStore.applyEvent({
            protocol_mode: "v2",
            category: "chat_state",
            state: "submitting",
            operation_id: "operation-accepted",
            command_id: command.command_id,
            request_id: command.request_id,
            correlation_id: command.correlation_id,
            conversation_id: "conversation-new",
            branch_id: "branch-new",
            turn_id: "turn-new"
        })
        compare(ChatStore.activeConversationId, "conversation-new")
        compare(ChatStore.activeBranchId, "branch-new")
        compare(ChatStore.activeTurnId, "turn-new")
        verify(!ChatStore.awaitingInitialScopeBinding)
    }

    function test_scoped_cancel_carries_target_operation_and_scope() {
        ChatStore.activeConversationId = "conversation-a"
        ChatStore.activeBranchId = "branch-a"
        ChatStore.activeOperationId = "operation-a"
        ChatStore.activeTurnId = "turn-a"

        verify(IntentDispatcher.dispatch({ type: "chat.stopGeneration" }))
        var command = JSON.parse(protocolAgent.lastCommandJson)
        compare(command.command_type, "chat.stop_generation")
        compare(command.operation_id, "operation-a")
        compare(command.scope.conversation_id, "conversation-a")
        compare(command.scope.branch_id, "branch-a")
        compare(command.scope.turn_id, "turn-a")
        compare(OperationStore.operations.get(OperationStore.findById("operation-a")).state, "cancelling")
    }

    function canonicalRustChatEnvelope(eventId, streamId, sequence, conversationId, branchId, state) {
        return {
            protocol_version: { major: 2, minor: 0 },
            event_id: eventId,
            stream_id: streamId,
            sequence: sequence,
            event_type: "chat_state",
            emitted_at: new Date(1760000000000 + sequence * 1000).toISOString(),
            payload: {
                schema_version: 1,
                category: "chat_state",
                state: state,
                conversation_id: conversationId,
                branch_id: branchId,
                turn_id: "turn-" + sequence
            }
        }
    }

    function test_rust_canonical_payload_scope_is_accepted_with_opaque_stream_id() {
        var streamId = "conversation:conversation-a:branch:branch-a"
        EventDispatcher.ingest(JSON.stringify(canonicalRustChatEnvelope(
                                                   "event-canonical-1", streamId, 1,
                                                   "conversation-a", "branch-a", "submitting")),
                               "agent")
        verify(EventDispatcher.lastEvent !== undefined)
        compare(EventDispatcher.lastEvent.conversation_id, "conversation-a")
        compare(EventDispatcher.lastEvent.branch_id, "branch-a")
        compare(EventDispatcher.lastEvent.stream_id, streamId)
        compare(EventDispatcher.lastSequenceByStream[streamId], 1)
    }

    function test_bound_stream_rejects_chat_scope_mutation() {
        var streamId = "opaque-chat-stream-17"
        EventDispatcher.ingest(JSON.stringify(canonicalRustChatEnvelope(
                                                   "event-scope-a", streamId, 1,
                                                   "conversation-a", "branch-a", "submitting")),
                               "agent")
        compare(EventDispatcher.lastSequenceByStream[streamId], 1)
        EventDispatcher.ingest(JSON.stringify(canonicalRustChatEnvelope(
                                                   "event-scope-b", streamId, 2,
                                                   "conversation-b", "branch-b", "streaming")),
                               "agent")
        compare(EventDispatcher.lastSequenceByStream[streamId], 1)
        compare(EventDispatcher.lastEvent.conversation_id, "conversation-a")
    }

    function test_top_level_only_chat_scope_is_rejected() {
        var envelope = canonicalRustChatEnvelope(
                    "event-top-level-only", "opaque-top-level-only", 1,
                    "conversation-a", "branch-a", "submitting")
        delete envelope.payload.conversation_id
        delete envelope.payload.branch_id
        envelope.conversation_id = "conversation-a"
        envelope.branch_id = "branch-a"
        EventDispatcher.ingest(JSON.stringify(envelope), "agent")
        verify(typeof EventDispatcher.lastSequenceByStream[envelope.stream_id] === "undefined")
    }

    function test_background_chat_event_cannot_hijack_active_scope() {
        ChatStore.activeConversationId = "conversation-a"
        ChatStore.activeBranchId = "branch-a"
        ChatStore.activeOperationId = "operation-a"

        ChatStore.applyEvent({
            protocol_mode: "v2",
            category: "chat_state",
            state: "streaming",
            operation_id: "operation-b",
            conversation_id: "conversation-b",
            branch_id: "branch-b",
            turn_id: "turn-b"
        })

        compare(ChatStore.activeConversationId, "conversation-a")
        compare(ChatStore.activeBranchId, "branch-a")
        verify(typeof ChatStore.dirtyBackgroundScopes[ChatStore.scopeKey("conversation-b", "branch-b")] !== "undefined")
    }
}
