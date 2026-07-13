from pathlib import Path

path = Path("qml/tests/tst_EventDispatcher.qml")
text = path.read_text(encoding="utf-8")

old_helper = '''    function v2ChatEvent(eventId, sequence, eventType, operationId, payload) {
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
    }'''
new_helper = '''    function v2ChatStreamId() {
        return "conversation:conversation-a:branch:branch-a"
    }

    function v2ChatEvent(eventId, sequence, eventType, operationId, payload) {
        var body = Object.assign({}, payload || ({}))
        body.conversation_id = "conversation-a"
        body.branch_id = "branch-a"
        body.turn_id = "turn-a"
        return {
            protocol_version: { major: 2, minor: 0 },
            event_id: eventId,
            stream_id: v2ChatStreamId(),
            sequence: sequence,
            event_type: eventType,
            emitted_at: new Date(1760000000000 + sequence * 1000).toISOString(),
            payload: body,
            correlation_id: "correlation-a",
            operation_id: operationId,
            request_id: "request-a",
            command_id: "command-a",
            command_type: "chat.send_message"
        }
    }'''
if text.count(old_helper) != 1:
    raise SystemExit(f"helper anchor mismatch: {text.count(old_helper)}")
text = text.replace(old_helper, new_helper, 1)

text = text.replace(
    'compare(EventDispatcher.lastSequenceByStream["chat:conversation-a:branch-a"], 2)',
    'compare(EventDispatcher.lastSequenceByStream[v2ChatStreamId()], 2)',
)
text = text.replace(
    'var streamId = "chat:conversation-a:branch-a"',
    'var streamId = v2ChatStreamId()',
)
text = text.replace(
    'delete event.branch_id',
    'delete event.payload.branch_id',
)

if 'stream_id: "chat:conversation-a:branch-a"' in text:
    raise SystemExit("stale chat stream fixture remains")
if 'delete event.branch_id' in text:
    raise SystemExit("stale top-level scope deletion remains")

path.write_text(text, encoding="utf-8")

for temporary in [
    ".github/fix_protocol_v2_fixture.py",
    ".github/workflows/protocol-v2-fixture-runner.yml",
]:
    candidate = Path(temporary)
    if candidate.exists():
        candidate.unlink()

print("Protocol V2 EventDispatcher fixtures migrated to canonical Rust wire shape")
