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

        if (!shouldAccept(event)) {
            return
        }

        lastEvent = event
        eventReceived(event)

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
            break
        default:
            break
        }
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
