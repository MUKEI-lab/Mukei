pragma Singleton
import QtQuick
import QtQml

Item {
    id: root

    property var agentSource: typeof mukeiAgent !== "undefined" ? mukeiAgent : null
    property var bridgeSource: typeof mukeiBridge !== "undefined" ? mukeiBridge : null
    property var lastEvent: undefined
    property var lastSequence: undefined

    signal eventReceived(var event)

    Component.onCompleted: reset()

    function reset() {
        lastEvent = undefined
        lastSequence = undefined
    }

    function ingest(eventJson) {
        var event
        try {
            event = JSON.parse(eventJson)
        } catch (error) {
            return
        }

        if (!shouldAccept(event) || !isKnownEnvelope(event)) {
            return
        }

        lastEvent = event
        eventReceived(event)

        routeKnownEvent(event)
    }

    function shouldAccept(event) {
        if (!event || event.schema_version !== 1) {
            return false
        }

        if (typeof event.sequence === "undefined" || event.sequence === null) {
            return true
        }

        if (typeof lastSequence !== "number" || event.sequence > lastSequence) {
            lastSequence = event.sequence
            return true
        }

        return false
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

    function isKnownEnvelope(event) {
        if (typeof event.category !== "string") {
            return false
        }

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
        case "download_failed":
        case "error":
            return hasError(event)
        default:
            return false
        }
    }

    function routeKnownEvent(event) {
        switch (event.category) {
        case "app_lifecycle":
        case "capability_snapshot":
        case "chat_state":
        case "chat_chunk":
        case "chat_completed":
        case "chat_cancelled":
        case "chat_failed":
        case "download_state":
        case "download_progress":
        case "download_completed":
        case "download_failed":
        case "error":
            return
        }
    }

    Connections {
        target: root.agentSource === null ? null : root.agentSource
        function onEvent_emitted(eventJson) {
            ingest(eventJson)
        }
    }

    Connections {
        target: root.bridgeSource === null ? null : root.bridgeSource
        function onEvent_emitted(eventJson) {
            ingest(eventJson)
        }
    }
}
