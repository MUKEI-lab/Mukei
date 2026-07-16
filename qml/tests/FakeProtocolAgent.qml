import QtQuick

QtObject {
    id: root

    property int sequence: 0
    property string signalMode: "snake"
    property bool emitLifecycleEvents: true
    property var submittedCommands: []

    signal event_emitted(string eventJson)
    signal eventEmitted(string eventJson)

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
                "command_envelope_v2", "command_acknowledgement"
            ],
            protocol: {
                current_version: { major: 2, minor: 0 },
                minimum_supported_peer_major: 2,
                capabilities: [
                    "command_envelope_v2", "command_acknowledgement",
                    "scoped_chat_operations", "event_identity",
                    "per_stream_sequencing", "operation_lifecycle_events"
                ]
            }
        })
    }

    function submit_command_json(rawCommand) {
        var command = JSON.parse(String(rawCommand))
        var commands = submittedCommands.slice(0)
        commands.push(command)
        submittedCommands = commands

        var acknowledgement = {
            protocol_version: { major: 2, minor: 0 },
            command_id: command.command_id,
            request_id: command.request_id,
            correlation_id: command.correlation_id,
            status: "accepted",
            operation_id: "operation-test-initialize",
            timestamp: new Date().toISOString()
        }

        if (command.command_type === "app.initialize" && emitLifecycleEvents) {
            Qt.callLater(function() {
                root.emitLifecycle("booting", command)
                root.emitLifecycle("loading_config", command)
                root.emitLifecycle("needs_database_key", command)
                root.emitLifecycle("opening_database", command)
                root.emitLifecycle("ready", command)
            })
        }
        return JSON.stringify(acknowledgement)
    }

    function emitLifecycle(state, command) {
        sequence += 1
        var envelope = {
            protocol_version: { major: 2, minor: 0 },
            event_id: "event-test-" + sequence,
            stream_id: "application:lifecycle",
            sequence: sequence,
            event_type: "app_lifecycle",
            emitted_at: new Date().toISOString(),
            command_id: command.command_id,
            request_id: command.request_id,
            correlation_id: command.correlation_id,
            command_type: command.command_type,
            operation_id: "operation-test-initialize",
            payload: {
                schema_version: 1,
                timestamp: new Date().toISOString(),
                category: "app_lifecycle",
                state: state,
                capabilities: state === "ready" ? {
                    can_initialize: false,
                    can_send_message: false,
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
                } : {
                    can_initialize: true,
                    can_send_message: false,
                    can_stop_generation: false,
                    can_download_model: false,
                    can_stop_download: false,
                    can_switch_model: false,
                    can_delete_model: false,
                    can_clear_conversation: false,
                    can_open_settings: true,
                    needs_config: false,
                    needs_storage_permission: false,
                    active_model_ready: false,
                    is_busy: true,
                    is_downloading: false,
                    is_inferencing: false
                },
                android_storage: { state: state === "ready" ? "ready" : "unknown" }
            }
        }
        var encoded = JSON.stringify(envelope)
        if (signalMode === "camel")
            eventEmitted(encoded)
        else
            event_emitted(encoded)
    }

    function reset() {
        sequence = 0
        submittedCommands = []
        emitLifecycleEvents = true
        signalMode = "snake"
    }
}
