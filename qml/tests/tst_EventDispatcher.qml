import QtQuick
import QtTest
import "../events"

TestCase {
    name: "EventDispatcher"

    SignalSpy {
        id: eventSpy
        target: EventDispatcher
        signalName: "eventReceived"
    }

    function init() {
        EventDispatcher.reset()
        eventSpy.clear()
    }

    function readyCapabilities() {
        return {
            can_initialize: false,
            can_send_message: true,
            can_stop_generation: false,
            can_download_model: true,
            can_stop_download: false,
            can_switch_model: true,
            can_delete_model: true,
            can_clear_conversation: true,
            can_open_settings: true,
            needs_config: false,
            needs_storage_permission: false,
            active_model_ready: false,
            is_busy: false,
            is_downloading: false,
            is_inferencing: false
        }
    }

    function test_accepts_chat_chunk_without_sequence() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "chat_chunk",
            chunk: "hello \"quoted\"\nworld"
        }))

        compare(eventSpy.count, 1)
        compare(EventDispatcher.lastEvent.category, "chat_chunk")
        compare(EventDispatcher.lastEvent.chunk, "hello \"quoted\"\nworld")
    }

    function test_rejects_schema_version_mismatch() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 999,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "chat_chunk",
            chunk: "ignored"
        }))

        compare(eventSpy.count, 0)
        verify(typeof EventDispatcher.lastEvent === "undefined")
    }

    function test_sequence_is_monotonic() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "chat_chunk",
            sequence: 10,
            chunk: "accepted"
        }))
        compare(eventSpy.count, 1)

        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "chat_chunk",
            sequence: 9,
            chunk: "rejected"
        }))

        compare(eventSpy.count, 1)
        compare(EventDispatcher.lastSequence, 10)
        compare(EventDispatcher.lastEvent.chunk, "accepted")
    }

    function test_missing_sequence_is_accepted() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "chat_chunk",
            chunk: "no sequence"
        }))

        compare(eventSpy.count, 1)
        compare(EventDispatcher.lastEvent.category, "chat_chunk")
        verify(typeof EventDispatcher.lastSequence === "undefined")
    }

    function test_null_sequence_is_accepted() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "chat_chunk",
            sequence: null,
            chunk: "null sequence"
        }))

        compare(eventSpy.count, 1)
        compare(EventDispatcher.lastEvent.category, "chat_chunk")
        verify(typeof EventDispatcher.lastSequence === "undefined")
    }

    function test_app_lifecycle_ready_is_accepted() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "app_lifecycle",
            state: "ready",
            capabilities: readyCapabilities()
        }))

        compare(eventSpy.count, 1)
        compare(EventDispatcher.lastEvent.category, "app_lifecycle")
        compare(EventDispatcher.lastEvent.state, "ready")
        compare(EventDispatcher.lastEvent.capabilities.can_send_message, true)
    }

    function test_unknown_event_type_is_rejected() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "future_event",
            payload: {}
        }))

        compare(eventSpy.count, 0)
        verify(typeof EventDispatcher.lastEvent === "undefined")
    }

    function test_malformed_json_is_rejected() {
        EventDispatcher.ingest("{not-json")

        compare(eventSpy.count, 0)
        verify(typeof EventDispatcher.lastEvent === "undefined")
    }

    function test_missing_required_payload_is_rejected() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "chat_chunk"
        }))

        compare(eventSpy.count, 0)
        verify(typeof EventDispatcher.lastEvent === "undefined")
    }

    function test_malformed_required_payload_is_rejected() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "download_progress",
            state: "downloading",
            progress: "0.5",
            bytes_downloaded: 128
        }))

        compare(eventSpy.count, 0)
        verify(typeof EventDispatcher.lastEvent === "undefined")
    }

    function test_error_event_requires_typed_error_payload() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "error",
            error: {
                code: "ERR_NETWORK",
                class: "network",
                severity: "error",
                recoverable: true
            }
        }))

        compare(eventSpy.count, 1)
        compare(EventDispatcher.lastEvent.error.code, "ERR_NETWORK")
    }

    function test_download_lifecycle_preserves_identity_and_ordering() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "download_state",
            sequence: 1,
            state: "queued",
            model_id: "gemma-4-e2b-it",
            destination: "model:gemma-4-e2b-it",
            capabilities: readyCapabilities()
        }))
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "download_progress",
            sequence: 2,
            state: "downloading",
            progress: 0.5,
            bytes_downloaded: 512,
            total_bytes: 1024,
            model_id: "gemma-4-e2b-it",
            destination: "model:gemma-4-e2b-it"
        }))
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "download_state",
            sequence: 1,
            state: "failed",
            model_id: "stale",
            destination: "model:stale",
            capabilities: readyCapabilities()
        }))

        compare(eventSpy.count, 2)
        compare(EventDispatcher.lastSequence, 2)
        compare(EventDispatcher.lastEvent.category, "download_progress")
        compare(EventDispatcher.lastEvent.model_id, "gemma-4-e2b-it")
        compare(EventDispatcher.lastEvent.destination, "model:gemma-4-e2b-it")
    }

    function test_ready_event_requires_capabilities() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "app_lifecycle",
            state: "ready"
        }))

        compare(eventSpy.count, 0)
        verify(typeof EventDispatcher.lastEvent === "undefined")
    }
    function test_download_completed_preserves_model_identity() {
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "download_completed",
            final_path: "model:gemma-4-e2b-it",
            model_id: "gemma-4-e2b-it"
        }))

        compare(eventSpy.count, 1)
        compare(EventDispatcher.lastEvent.model_id, "gemma-4-e2b-it")
        compare(EventDispatcher.lastEvent.final_path, "model:gemma-4-e2b-it")
    }

    function v2ChatEvent(eventId, sequence, eventType, operationId, payload) {
        return {
            protocol_version: { major: 2, minor: 0 },
            event_id: eventId,
            stream_id: "chat:conversation-a:branch-a",
            sequence: sequence,
            event_type: eventType,
            emitted_at: "2026-07-11T13:00:00.000Z",
            payload: payload || ({}),
            correlation_id: "correlation-a",
            operation_id: operationId,
            request_id: "request-a",
            command_id: "command-a",
            command_type: "chat_send_message",
            conversation_id: "conversation-a",
            branch_id: "branch-a",
            turn_id: "turn-a"
        }
    }

    function test_v2_duplicate_event_id_is_rejected() {
        var first = v2ChatEvent("event-v2-1", 1, "chat_state", "operation-a", { state: "submitting" })
        EventDispatcher.ingest(JSON.stringify(first))
        EventDispatcher.ingest(JSON.stringify(first))
        compare(eventSpy.count, 1)
    }

    function test_v2_stale_sequence_is_rejected_per_stream() {
        EventDispatcher.ingest(JSON.stringify(
            v2ChatEvent("event-v2-stale-2", 2, "chat_state", "operation-a", { state: "submitting" })))
        EventDispatcher.ingest(JSON.stringify(
            v2ChatEvent("event-v2-stale-1", 1, "chat_chunk", "operation-a", { chunk: "late" })))
        compare(eventSpy.count, 1)
        compare(EventDispatcher.lastSequenceByStream["chat:conversation-a:branch-a"], 2)
    }

    function test_v2_gap_is_quarantined_until_snapshot_resync() {
        EventDispatcher.ingest(JSON.stringify(
            v2ChatEvent("event-v2-gap-1", 1, "chat_state", "operation-a", { state: "submitting" })))
        EventDispatcher.ingest(JSON.stringify(
            v2ChatEvent("event-v2-gap-3", 3, "chat_chunk", "operation-a", { chunk: "gap" })))
        compare(eventSpy.count, 1)
        verify(EventDispatcher.uncertainStreams["chat:conversation-a:branch-a"] === true)

        EventDispatcher.ingest(JSON.stringify(
            v2ChatEvent("event-v2-gap-4", 4, "chat_chunk", "operation-a", { chunk: "blocked" })))
        compare(eventSpy.count, 1)

        verify(EventDispatcher.markChatScopeResynchronized("conversation-a", "branch-a"))
        EventDispatcher.ingest(JSON.stringify(
            v2ChatEvent("event-v2-gap-4", 4, "chat_chunk", "operation-a", { chunk: "accepted" })))
        compare(eventSpy.count, 2)
    }

    function test_v2_duplicate_logical_terminal_is_rejected() {
        EventDispatcher.ingest(JSON.stringify(
            v2ChatEvent("event-v2-terminal-1", 1, "chat_completed", "operation-a", {})))
        EventDispatcher.ingest(JSON.stringify(
            v2ChatEvent("event-v2-terminal-2", 2, "chat_completed", "operation-a", {})))
        compare(eventSpy.count, 1)
    }

    function test_v2_progress_after_terminal_is_rejected() {
        EventDispatcher.ingest(JSON.stringify(
            v2ChatEvent("event-v2-finished", 1, "chat_completed", "operation-a", {})))
        EventDispatcher.ingest(JSON.stringify(
            v2ChatEvent("event-v2-late-progress", 2, "chat_chunk", "operation-a", { chunk: "late" })))
        compare(eventSpy.count, 1)
    }

    function test_v2_missing_chat_scope_fails_closed() {
        var event = v2ChatEvent("event-v2-missing-scope", 1, "chat_chunk", "operation-a", { chunk: "x" })
        delete event.branch_id
        EventDispatcher.ingest(JSON.stringify(event))
        compare(eventSpy.count, 0)
    }

    function test_v2_unsupported_protocol_major_fails_closed() {
        var event = v2ChatEvent("event-v2-bad-major", 1, "chat_chunk", "operation-a", { chunk: "x" })
        event.protocol_version.major = 99
        EventDispatcher.ingest(JSON.stringify(event))
        compare(eventSpy.count, 0)
    }

    function test_v2_operation_event_requires_operation_local_stream() {
        var event = {
            protocol_version: { major: 2, minor: 0 },
            event_id: "event-operation-bad-stream",
            stream_id: "app",
            sequence: 1,
            event_type: "operation_started",
            emitted_at: "2026-07-11T13:00:00.000Z",
            payload: {},
            operation_id: "operation-a"
        }
        EventDispatcher.ingest(JSON.stringify(event))
        compare(eventSpy.count, 0)
    }

}
