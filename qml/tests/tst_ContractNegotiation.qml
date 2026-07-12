import QtQuick
import QtTest
import "../stores"

TestCase {
    name: "ContractNegotiation"

    function protocol(capabilities) {
        return {
            current_version: { major: 2, minor: 0 },
            minimum_supported_peer_major: 2,
            capabilities: capabilities
        }
    }

    readonly property var baseFeatures: [
        "typed_commands", "typed_events", "snapshot_delta_sync",
        "persistent_ui_session", "capability_gating",
        "command_envelope_v2", "command_acknowledgement",
        "event_identity", "per_stream_sequencing",
        "idempotent_command_replay", "operation_lifecycle_events",
        "scoped_chat_operations", "legacy_event_v1_compatibility"
    ]

    readonly property var v2Capabilities: [
        "command_envelope_v2", "command_acknowledgement",
        "event_identity", "per_stream_sequencing",
        "idempotent_command_replay", "operation_lifecycle_events",
        "scoped_chat_operations", "legacy_event_v1_compatibility"
    ]

    QtObject {
        id: compatibleAgent
        function submit_command_json(commandJson) { return "{}" }
        function ui_contract_snapshot_json() {
            return JSON.stringify({
                schema_version: 1,
                contract_version: 1,
                min_qml_contract_version: 1,
                max_qml_contract_version: 1,
                command_schema_version: 2,
                event_schema_version: 2,
                snapshot_schema_version: 1,
                required_features: baseFeatures,
                protocol: protocol(v2Capabilities)
            })
        }
    }

    QtObject {
        id: incompatibleAgent
        function ui_contract_snapshot_json() {
            return JSON.stringify({
                schema_version: 1,
                contract_version: 2,
                min_qml_contract_version: 2,
                max_qml_contract_version: 2,
                command_schema_version: 2,
                event_schema_version: 2,
                snapshot_schema_version: 1,
                required_features: baseFeatures,
                protocol: protocol(v2Capabilities)
            })
        }
    }

    QtObject {
        id: missingFeatureAgent
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
                    "persistent_ui_session", "command_envelope_v2",
                    "command_acknowledgement", "event_identity",
                    "per_stream_sequencing", "operation_lifecycle_events"
                ],
                protocol: protocol(v2Capabilities)
            })
        }
    }

    QtObject {
        id: unknownFeatureAgent
        function ui_contract_snapshot_json() {
            var features = baseFeatures.slice(0)
            features.push("future_unknown_feature")
            return JSON.stringify({
                schema_version: 1,
                contract_version: 1,
                min_qml_contract_version: 1,
                max_qml_contract_version: 1,
                command_schema_version: 2,
                event_schema_version: 2,
                snapshot_schema_version: 1,
                required_features: features,
                protocol: protocol(v2Capabilities)
            })
        }
    }

    QtObject {
        id: legacyEventAgent
        function ui_contract_snapshot_json() {
            return JSON.stringify({
                schema_version: 1,
                contract_version: 1,
                min_qml_contract_version: 1,
                max_qml_contract_version: 1,
                command_schema_version: 2,
                event_schema_version: 1,
                snapshot_schema_version: 1,
                required_features: [
                    "typed_commands", "typed_events", "snapshot_delta_sync",
                    "persistent_ui_session", "capability_gating",
                    "command_envelope_v2", "command_acknowledgement",
                    "operation_lifecycle_events", "legacy_event_v1_compatibility"
                ],
                protocol: protocol([
                    "command_envelope_v2", "command_acknowledgement",
                    "operation_lifecycle_events", "scoped_chat_operations", "legacy_event_v1_compatibility"
                ])
            })
        }
    }

    function cleanup() {
        ContractStore.reset()
    }

    function test_accepts_matching_contract() {
        ContractStore.configure(compatibleAgent)
        verify(ContractStore.hydrate())
        verify(ContractStore.compatible)
        compare(ContractStore.bridgeContractVersion, 1)
        compare(ContractStore.eventMode, "protocol_v2")
        compare(ContractStore.protocolMode, "v2")
        verify(ContractStore.protocolV2Available)
        verify(ContractStore.scopedCancellationAvailable)
    }

    function test_accepts_isolated_legacy_event_transition_mode() {
        ContractStore.configure(legacyEventAgent)
        verify(ContractStore.hydrate())
        verify(ContractStore.compatible)
        compare(ContractStore.eventMode, "legacy_v1")
    }

    function test_rejects_incompatible_contract_before_startup() {
        ContractStore.configure(incompatibleAgent)
        verify(!ContractStore.hydrate())
        verify(!ContractStore.compatible)
        verify(ContractStore.safeMessage.length > 0)
    }

    function test_rejects_missing_required_baseline_feature() {
        ContractStore.configure(missingFeatureAgent)
        verify(!ContractStore.hydrate())
        verify(!ContractStore.compatible)
    }

    function test_rejects_unknown_bridge_required_feature() {
        ContractStore.configure(unknownFeatureAgent)
        verify(!ContractStore.hydrate())
        verify(!ContractStore.compatible)
    }
}
