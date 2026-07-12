pragma Singleton
import QtQuick
import QtQml

Item {
    id: root

    property var agentSource: typeof mukeiAgent !== "undefined" ? mukeiAgent : null
    property var bridgeSource: typeof mukeiBridge !== "undefined" ? mukeiBridge : null
    property var lastEvent: undefined
    property var lastSequence: undefined
    property var lastSequenceBySource: ({})
    property var lastSequenceByStream: ({})
    property var trackedStreamOrder: []
    property var acceptedEventIds: ({})
    property var acceptedEventOrder: []
    property var acceptedLogicalEvents: ({})
    property var acceptedLogicalOrder: []
    property var acceptedTerminalKeys: ({})
    property var acceptedTerminalOrder: []

    signal eventReceived(var event)
    signal sequenceGapDetected(string feature, int expectedSequence, int receivedSequence)
    signal streamSequenceGapDetected(string feature, string streamId, double expectedSequence, double receivedSequence)
    signal eventRejected(string reason)

    Component.onCompleted: reset()

    function reset() {
        lastEvent = undefined
        lastSequence = undefined
        lastSequenceBySource = ({})
        lastSequenceByStream = ({})
        trackedStreamOrder = []
        acceptedEventIds = ({})
        acceptedEventOrder = []
        acceptedLogicalEvents = ({})
        acceptedLogicalOrder = []
        acceptedTerminalKeys = ({})
        acceptedTerminalOrder = []
    }

    function ingest(eventJson, sourceName) {
        if (typeof eventJson !== "string" || eventJson.length === 0 || eventJson.length > 1048576) {
            eventRejected("invalid_event_size")
            return
        }
        var raw
        try {
            raw = JSON.parse(eventJson)
        } catch (error) {
            eventRejected("malformed_json")
            return
        }

        var event
        if (isObject(raw) && isObject(raw.protocol_version)) {
            if (Number(raw.protocol_version.major) !== 2) {
                eventRejected("unsupported_protocol_major")
                return
            }
            if (!isV2Envelope(raw)) {
                eventRejected("unknown_or_invalid_v2_envelope")
                return
            }
            event = normalizeV2(raw)
            if (!event || !isKnownPayload(event)) {
                eventRejected("unknown_or_invalid_v2_envelope")
                return
            }
            if (!shouldAcceptV2(raw, event))
                return
        } else {
            event = normalizeLegacyV1(raw)
            if (!event) {
                eventRejected("unknown_or_invalid_envelope")
                return
            }
            if (!shouldAcceptLegacy(event, sourceName || "manual"))
                return
        }

        if (!rememberLogicalTerminal(event)) {
            eventRejected("duplicate_terminal_event")
            return
        }
        if (!rememberLogicalEvent(event)) {
            eventRejected("duplicate_logical_event")
            return
        }
        lastEvent = event
        eventReceived(event)
    }

    function isV2Envelope(value) {
        return isObject(value)
                && isObject(value.protocol_version)
                && Number(value.protocol_version.major) === 2
                && typeof value.event_id === "string"
                && value.event_id.length > 0
                && value.event_id.length <= 128
                && typeof value.stream_id === "string"
                && value.stream_id.length > 0
                && value.stream_id.length <= 256
                && typeof value.sequence === "number"
                && Number.isFinite(value.sequence)
                && Math.floor(value.sequence) === value.sequence
                && value.sequence >= 1
                && typeof value.event_type === "string"
                && value.event_type.length > 0
                && value.event_type.length <= 96
                && typeof value.emitted_at === "string"
                && value.emitted_at.length > 0
                && isObject(value.payload)
    }

    function normalizeV2(raw) {
        if (Number(raw.protocol_version.major) !== 2) {
            eventRejected("unsupported_protocol_major")
            return null
        }
        var payloadCategory = typeof raw.payload.category === "string" ? raw.payload.category : raw.event_type
        if (payloadCategory !== raw.event_type)
            return null
        var event = Object.assign({}, raw.payload)
        event.category = raw.event_type
        event.protocol_version = raw.protocol_version
        event.event_id = raw.event_id
        event.stream_id = raw.stream_id
        event.sequence = Number(raw.sequence)
        event.emitted_at = raw.emitted_at || ""
        event.correlation_id = raw.correlation_id || ""
        event.operation_id = raw.operation_id || ""
        event.request_id = raw.request_id || ""
        event.command_id = raw.command_id || ""
        event.command_type = raw.command_type || ""
        event.protocol_mode = "v2"
        return event
    }

    function normalizeLegacyV1(raw) {
        if (!raw || Number(raw.schema_version) !== 1 || !isKnownPayload(raw))
            return null
        var event = Object.assign({}, raw)
        event.protocol_mode = "legacy_v1"
        event.stream_id = "legacy:" + featureForCategory(event.category)
        return event
    }

    function featureForCategory(category) {
        if (category.indexOf("chat_") === 0)
            return "chat"
        if (category.indexOf("download_") === 0)
            return "downloads"
        if (category.indexOf("document_") === 0)
            return "documents"
        if (category === "app_lifecycle" || category === "capability_snapshot")
            return "app"
        if (category === "operation_lifecycle")
            return "operations"
        return "errors"
    }

    function rememberEventId(eventId) {
        if (acceptedEventIds[eventId] === true)
            return false
        var ids = Object.assign({}, acceptedEventIds)
        var order = acceptedEventOrder.slice(0)
        ids[eventId] = true
        order.push(eventId)
        while (order.length > 512) {
            var expired = order.shift()
            delete ids[expired]
        }
        acceptedEventIds = ids
        acceptedEventOrder = order
        return true
    }

    function rememberStreamSequence(streamId, sequence) {
        var next = Object.assign({}, lastSequenceByStream)
        var order = trackedStreamOrder.slice(0)
        var existingIndex = order.indexOf(streamId)
        if (existingIndex >= 0)
            order.splice(existingIndex, 1)
        next[streamId] = sequence
        order.push(streamId)
        while (order.length > 256) {
            var expired = order.shift()
            delete next[expired]
        }
        lastSequenceByStream = next
        trackedStreamOrder = order
    }

    function shouldAcceptV2(raw, event) {
        if (!rememberEventId(raw.event_id)) {
            eventRejected("duplicate_event")
            return false
        }
        var previous = lastSequenceByStream[raw.stream_id]
        if (typeof previous === "number") {
            if (raw.sequence <= previous) {
                eventRejected("stale_sequence")
                return false
            }
            if (raw.sequence > previous + 1) {
                var feature = featureForCategory(event.category)
                sequenceGapDetected(feature, previous + 1, raw.sequence)
                streamSequenceGapDetected(feature, raw.stream_id, previous + 1, raw.sequence)
                eventRejected("sequence_gap_resync_required")
                return false
            }
        }
        rememberStreamSequence(raw.stream_id, raw.sequence)
        lastSequence = typeof lastSequence === "number" ? Math.max(lastSequence, raw.sequence) : raw.sequence
        return true
    }

    function completeResynchronization(streamId, baselineSequence) {
        if (!streamId)
            return
        rememberStreamSequence(streamId, Number(baselineSequence || 0))
    }

    function shouldAcceptLegacy(event, sourceName) {
        if (typeof event.event_id === "string" && event.event_id.length > 0) {
            if (!rememberEventId(event.event_id)) {
                eventRejected("duplicate_event")
                return false
            }
        }
        if (typeof event.sequence === "undefined" || event.sequence === null)
            return true

        var previous = lastSequenceBySource[sourceName]
        if (typeof previous === "number") {
            if (event.sequence <= previous) {
                eventRejected("stale_sequence")
                return false
            }
            if (event.sequence > previous + 1)
                sequenceGapDetected(featureForCategory(event.category), previous + 1, event.sequence)
        }

        var next = Object.assign({}, lastSequenceBySource)
        next[sourceName] = event.sequence
        lastSequenceBySource = next
        lastSequence = typeof lastSequence === "number" ? Math.max(lastSequence, event.sequence) : event.sequence
        return true
    }

    function terminalCategory(event) {
        if (!event)
            return ""
        if (event.category === "chat_completed")
            return "completed"
        if (event.category === "chat_cancelled")
            return "cancelled"
        if (event.category === "chat_failed")
            return "failed"
        if (event.category === "operation_lifecycle"
                && ["completed", "succeeded", "failed", "cancelled", "rejected"].indexOf(event.state) >= 0)
            return event.state === "succeeded" ? "completed" : event.state
        if (event.category === "chat_state"
                && ["completed", "failed", "cancelled"].indexOf(event.state) >= 0)
            return event.state
        return ""
    }

    function rememberLogicalTerminal(event) {
        var terminal = terminalCategory(event)
        if (!terminal || !event.operation_id)
            return true
        var key = String(event.operation_id) + "|" + terminal
        if (acceptedTerminalKeys[key] === true)
            return false
        var seen = Object.assign({}, acceptedTerminalKeys)
        var order = acceptedTerminalOrder.slice(0)
        seen[key] = true
        order.push(key)
        while (order.length > 512) {
            var expired = order.shift()
            delete seen[expired]
        }
        acceptedTerminalKeys = seen
        acceptedTerminalOrder = order
        return true
    }

    function logicalFingerprint(event) {
        var errorCode = event.error && event.error.code ? event.error.code : ""
        return [
            event.category || "", event.timestamp || event.emitted_at || "",
            event.conversation_id || "", event.branch_id || "", event.turn_id || "",
            event.message_id || "", event.model_id || "", event.destination || "",
            event.state || "", event.chunk || "", event.final_path || "", errorCode
        ].join("|")
    }

    function rememberLogicalEvent(event) {
        var fingerprint = logicalFingerprint(event)
        if (!fingerprint)
            return true
        if (acceptedLogicalEvents[fingerprint] === true)
            return false
        var seen = Object.assign({}, acceptedLogicalEvents)
        var order = acceptedLogicalOrder.slice(0)
        seen[fingerprint] = true
        order.push(fingerprint)
        while (order.length > 512) {
            var expired = order.shift()
            delete seen[expired]
        }
        acceptedLogicalEvents = seen
        acceptedLogicalOrder = order
        return true
    }

    function isObject(value) {
        return value !== null && typeof value === "object" && !Array.isArray(value)
    }

    function isOptionalString(value) {
        return typeof value === "undefined" || value === null || typeof value === "string"
    }

    function hasCapabilities(event) {
        return isObject(event.capabilities)
    }

    function hasError(event) {
        return isObject(event.error) && typeof event.error.code === "string"
    }

    function isKnownPayload(event) {
        if (!event || typeof event.category !== "string")
            return false

        switch (event.category) {
        case "app_lifecycle":
            return typeof event.state === "string" && hasCapabilities(event)
        case "capability_snapshot":
            return hasCapabilities(event)
        case "chat_state":
            return typeof event.state === "string" && hasCapabilities(event)
        case "chat_chunk":
            return typeof event.chunk === "string"
        case "chat_completed":
        case "chat_cancelled":
            return true
        case "chat_failed":
            return hasError(event)
        case "download_state":
            return typeof event.state === "string"
                    && hasCapabilities(event)
                    && isOptionalString(event.model_id)
                    && isOptionalString(event.destination)
        case "download_progress":
            return typeof event.state === "string"
                    && typeof event.progress === "number"
                    && typeof event.bytes_downloaded === "number"
                    && (typeof event.total_bytes === "undefined"
                        || event.total_bytes === null
                        || typeof event.total_bytes === "number")
                    && isOptionalString(event.model_id)
                    && isOptionalString(event.destination)
        case "download_completed":
            return typeof event.final_path === "string"
                    && isOptionalString(event.model_id)
        case "download_failed":
        case "error":
            return hasError(event)
        case "operation_lifecycle":
            return typeof event.state === "string"
                    && ["completed", "failed", "cancelled", "running", "accepted"].indexOf(event.state) >= 0
        default:
            return false
        }
    }

    Connections {
        target: root.agentSource === null ? null : root.agentSource
        function onEvent_emitted(eventJson) { root.ingest(eventJson, "agent") }
    }

    Connections {
        target: root.bridgeSource === null ? null : root.bridgeSource
        function onEvent_emitted(eventJson) { root.ingest(eventJson, "bridge") }
    }
}
