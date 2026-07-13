from pathlib import Path


def replace_once(path: str, old: str, new: str, label: str) -> None:
    file_path = Path(path)
    text = file_path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one anchor, found {count}")
    file_path.write_text(text.replace(old, new, 1), encoding="utf-8")
    print(f"PASS {label}")


# QML consumes the actual Rust EventEnvelopeV2 wire shape: domain scope is
# authoritative inside payload; stream_id is an opaque sequencing identity.
replace_once(
    "qml/events/EventDispatcher.qml",
    "    property var lastSequenceByStream: ({})\n    property var trackedStreamOrder: []\n    property var uncertainStreams: ({})",
    "    property var lastSequenceByStream: ({})\n    property var trackedStreamOrder: []\n    property var streamScopeById: ({})\n    property var uncertainStreams: ({})",
    "EventDispatcher stream-scope state",
)
replace_once(
    "qml/events/EventDispatcher.qml",
    "        lastSequenceByStream = ({})\n        trackedStreamOrder = []\n        uncertainStreams = ({})",
    "        lastSequenceByStream = ({})\n        trackedStreamOrder = []\n        streamScopeById = ({})\n        uncertainStreams = ({})",
    "EventDispatcher stream-scope reset",
)
replace_once(
    "qml/events/EventDispatcher.qml",
    '''        event.command_type = raw.command_type || ""
        event.conversation_id = raw.conversation_id || event.conversation_id || ""
        event.branch_id = raw.branch_id || event.branch_id || ""
        event.turn_id = raw.turn_id || event.turn_id || ""
        event.protocol_mode = "v2"''',
    '''        event.command_type = raw.command_type || ""
        event.conversation_id = event.conversation_id || ""
        event.branch_id = event.branch_id || ""
        event.turn_id = event.turn_id || ""
        event.protocol_mode = "v2"''',
    "EventDispatcher payload-authoritative normalization",
)
replace_once(
    "qml/events/EventDispatcher.qml",
    '''    function validateV2Scope(raw, event) {
        if (!isChatCategory(event.category))
            return true
        if (typeof raw.conversation_id !== "string" || raw.conversation_id.length === 0
                || typeof raw.branch_id !== "string" || raw.branch_id.length === 0) {
            eventRejected("missing_chat_scope")
            return false
        }
        var expectedStreamId = "chat:" + raw.conversation_id + ":" + raw.branch_id
        if (raw.stream_id !== expectedStreamId) {
            eventRejected("chat_stream_scope_mismatch")
            return false
        }
        return true
    }
''',
    '''    function chatScopeKey(conversationId, branchId) {
        return conversationId.length + ":" + conversationId + ":" + branchId
    }

    function validateV2Scope(raw, event) {
        if (!isChatCategory(event.category))
            return true
        if (typeof event.conversation_id !== "string" || event.conversation_id.length === 0
                || typeof event.branch_id !== "string" || event.branch_id.length === 0) {
            eventRejected("missing_chat_scope")
            return false
        }
        var boundScope = streamScopeById[raw.stream_id]
        var eventScope = chatScopeKey(event.conversation_id, event.branch_id)
        if (typeof boundScope === "string" && boundScope !== eventScope) {
            eventRejected("stream_scope_mutation")
            return false
        }
        return true
    }

    function rememberV2StreamScope(streamId, event) {
        if (!isChatCategory(event.category))
            return
        var scopes = Object.assign({}, streamScopeById)
        scopes[streamId] = chatScopeKey(event.conversation_id, event.branch_id)
        streamScopeById = scopes
    }

    function streamIdForChatScope(conversationId, branchId) {
        if (!conversationId || !branchId)
            return ""
        var expected = chatScopeKey(conversationId, branchId)
        var streamIds = Object.keys(streamScopeById)
        for (var i = 0; i < streamIds.length; ++i) {
            if (streamScopeById[streamIds[i]] === expected)
                return streamIds[i]
        }
        return ""
    }
''',
    "EventDispatcher opaque-stream scope validation",
)
replace_once(
    "qml/events/EventDispatcher.qml",
    '''        var next = Object.assign({}, lastSequenceByStream)
        var order = trackedStreamOrder.slice(0)
        var existingIndex = order.indexOf(streamId)''',
    '''        var next = Object.assign({}, lastSequenceByStream)
        var scopes = Object.assign({}, streamScopeById)
        var order = trackedStreamOrder.slice(0)
        var existingIndex = order.indexOf(streamId)''',
    "EventDispatcher bounded scope map",
)
replace_once(
    "qml/events/EventDispatcher.qml",
    '''        while (order.length > 256) {
            var expired = order.shift()
            delete next[expired]
        }
        lastSequenceByStream = next
        trackedStreamOrder = order''',
    '''        while (order.length > 256) {
            var expired = order.shift()
            delete next[expired]
            delete scopes[expired]
        }
        lastSequenceByStream = next
        streamScopeById = scopes
        trackedStreamOrder = order''',
    "EventDispatcher scope eviction",
)
replace_once(
    "qml/events/EventDispatcher.qml",
    '''        rememberStreamSequence(raw.stream_id, raw.sequence)
        lastSequence = typeof lastSequence === "number" ? Math.max(lastSequence, raw.sequence) : raw.sequence
        return true''',
    '''        rememberStreamSequence(raw.stream_id, raw.sequence)
        rememberV2StreamScope(raw.stream_id, event)
        lastSequence = typeof lastSequence === "number" ? Math.max(lastSequence, raw.sequence) : raw.sequence
        return true''',
    "EventDispatcher accepted stream binding",
)
replace_once(
    "qml/events/EventDispatcher.qml",
    '''    function markChatScopeResynchronized(conversationId, branchId, baselineSequence, validatedByController) {
        if (validatedByController !== true || !conversationId || !branchId)
            return false
        var streamId = "chat:" + conversationId + ":" + branchId
        return completeResynchronization(streamId, baselineSequence)
    }''',
    '''    function markChatScopeResynchronized(conversationId, branchId, baselineSequence, validatedByController) {
        if (validatedByController !== true || !conversationId || !branchId)
            return false
        var streamId = streamIdForChatScope(conversationId, branchId)
        return streamId.length > 0 && completeResynchronization(streamId, baselineSequence)
    }''',
    "EventDispatcher opaque-stream resync compatibility",
)

# Remove obsolete duplicate resync completion attempts from ChatStore. The
# authoritative completion owner is AppCoordinator after snapshotApplied.
replace_once(
    "qml/stores/ChatStore.qml",
    '''            if (applied) {
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
            }''',
    '''            if (applied) {
                UiSessionStore.setTimelineAnchor(parsed.oldest_message_id || "")
                if (!prepend)
                    clearBackgroundScopeDirty(activeConversationId, activeBranchId)
                // AppCoordinator owns correlated resync completion after this
                // successful store-apply signal; the store never reopens a stream.
                snapshotApplied()
                tailUpdated()
            }''',
    "ChatStore single resync completion owner",
)

# QML consumer conformance tests use the exact current Rust producer shape.
p = Path("qml/tests/tst_ProtocolV2.qml")
text = p.read_text(encoding="utf-8")
marker = "    function test_background_chat_event_cannot_hijack_active_scope() {"
if text.count(marker) != 1:
    raise SystemExit(f"ProtocolV2 test insertion: expected one anchor, found {text.count(marker)}")
insert = '''    function canonicalRustChatEnvelope(eventId, streamId, sequence, conversationId, branchId, state) {
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

'''
p.write_text(text.replace(marker, insert + marker, 1), encoding="utf-8")
print("PASS ProtocolV2 QML consumer conformance tests")

# Rust producer conformance test locks the actual serialized event shape.
p = Path("rust/crates/mukei-bridge/src/protocol.rs")
text = p.read_text(encoding="utf-8")
test_marker = "canonical_chat_event_serialization_keeps_scope_in_payload"
if test_marker in text:
    raise SystemExit("Rust producer conformance test already exists")
text += r'''

#[cfg(test)]
mod canonical_wire_tests {
    use super::*;
    use mukei_core::types::{BranchId, ConversationId};
    use mukei_core::ui_contract::{CapabilitySnapshot, ChatTurnState};

    #[test]
    fn canonical_chat_event_serialization_keeps_scope_in_payload() {
        let conversation = ConversationId::new();
        let branch = BranchId::new();
        let event = BridgeEvent::new(BridgeEventKind::ChatState {
            state: ChatTurnState::Submitting,
            capabilities: CapabilitySnapshot::inferencing(),
        })
        .with_chat_scope(conversation, branch, "turn-wire-contract".to_string());

        let serialized = ProtocolRuntimeState::new().wrap_bridge_event(event);
        let value: serde_json::Value = serde_json::from_str(&serialized).expect("valid event json");
        let stream_id = value["stream_id"].as_str().expect("stream id");

        assert_eq!(value["protocol_version"]["major"], 2);
        assert_eq!(value["event_type"], "chat_state");
        assert_eq!(value["payload"]["conversation_id"], conversation.0.to_string());
        assert_eq!(value["payload"]["branch_id"], branch.0.to_string());
        assert_eq!(value["payload"]["turn_id"], "turn-wire-contract");
        assert!(value.get("conversation_id").is_none());
        assert!(value.get("branch_id").is_none());
        assert!(stream_id.contains(&conversation.0.to_string()));
        assert!(stream_id.contains(&branch.0.to_string()));
    }
}
'''
p.write_text(text, encoding="utf-8")
print("PASS Rust producer conformance test")

# Remove every temporary patch transport file from the production commit.
for temporary in [
    ".github/workflows/protocol-v2-contract-patcher.yml",
    ".github/workflows/protocol-v2-contract-trigger.yml",
    ".github/workflows/protocol-v2-contract-runner.yml",
    ".github/protocol_v2_contract_patch.py",
]:
    path = Path(temporary)
    if path.exists():
        path.unlink()
        print(f"PASS removed temporary {temporary}")

print("Protocol V2 canonical wire contract patch complete")
