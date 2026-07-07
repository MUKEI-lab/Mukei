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
            destination: "/models/gemma.gguf",
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
            destination: "/models/gemma.gguf"
        }))
        EventDispatcher.ingest(JSON.stringify({
            schema_version: 1,
            timestamp: "2026-07-04T13:00:00.000Z",
            category: "download_state",
            sequence: 1,
            state: "failed",
            model_id: "stale",
            destination: "/models/stale.gguf",
            capabilities: readyCapabilities()
        }))

        compare(eventSpy.count, 2)
        compare(EventDispatcher.lastSequence, 2)
        compare(EventDispatcher.lastEvent.category, "download_progress")
        compare(EventDispatcher.lastEvent.model_id, "gemma-4-e2b-it")
        compare(EventDispatcher.lastEvent.destination, "/models/gemma.gguf")
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
}
