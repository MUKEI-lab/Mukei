pragma Singleton
import QtQuick

QtObject {
    readonly property int qmlContractVersion: 1
    readonly property int protocolMajor: 2
    property var agentSource: null
    property bool hydrated: false
    property bool compatible: false
    property int bridgeContractVersion: 0
    property int minimumQmlVersion: 0
    property int maximumQmlVersion: 0
    property int commandSchemaVersion: 0
    property int eventSchemaVersion: 0
    property int snapshotSchemaVersion: 0
    property int bridgeProtocolMajor: 0
    property int bridgeProtocolMinor: 0
    property int minimumPeerProtocolMajor: 0
    property var protocolCapabilities: []
    property string eventMode: "unknown"
    property bool protocolTransportAvailable: false
    property string protocolMode: "unknown"
    readonly property bool protocolV2Available: compatible
            && protocolMode === "v2"
            && protocolTransportAvailable
    readonly property bool authoritativeAcknowledgements: protocolV2Available
            && protocolCapabilities.indexOf("command_acknowledgement") >= 0
    readonly property bool scopedCancellationAvailable: protocolV2Available
            && protocolCapabilities.indexOf("scoped_chat_operations") >= 0
    readonly property bool eventStreamReliabilityAvailable: protocolV2Available
            && eventMode === "protocol_v2"
    readonly property var supportedFeatures: [
        "typed_commands", "typed_events", "snapshot_delta_sync",
        "persistent_ui_session", "capability_gating",
        "command_envelope_v2", "command_acknowledgement", "event_identity",
        "per_stream_sequencing", "idempotent_command_replay",
        "operation_lifecycle_events", "scoped_chat_operations",
        "legacy_event_v1_compatibility"
    ]
    property var requiredFeatures: []
    property string safeMessage: qsTr("The frontend and local service have not negotiated a compatible contract yet.")

    signal negotiationCompleted(bool compatible)

    function configure(agent) {
        agentSource = agent
        protocolTransportAvailable = agentSource !== null
                && typeof agentSource.submit_command_json === "function"
    }

    function setProtocolTransportAvailable(available) {
        protocolTransportAvailable = available === true
        recomputeProtocolMode()
    }

    function supportsProtocolFeature(feature) {
        return protocolV2Available && protocolCapabilities.indexOf(feature) >= 0
    }

    function recomputeProtocolMode() {
        if (!compatible) {
            protocolMode = hydrated ? "incompatible" : "unknown"
            return
        }
        protocolMode = protocolTransportAvailable ? "v2" : "partial_v2"
    }

    function reset() {
        hydrated = false
        compatible = false
        bridgeContractVersion = 0
        minimumQmlVersion = 0
        maximumQmlVersion = 0
        commandSchemaVersion = 0
        eventSchemaVersion = 0
        snapshotSchemaVersion = 0
        bridgeProtocolMajor = 0
        bridgeProtocolMinor = 0
        minimumPeerProtocolMajor = 0
        protocolCapabilities = []
        eventMode = "unknown"
        protocolMode = "unknown"
        requiredFeatures = []
    }

    function containsAll(haystack, needles) {
        for (var i = 0; i < needles.length; ++i)
            if (haystack.indexOf(needles[i]) < 0)
                return false
        return true
    }

    function hydrate() {
        reset()
        if (agentSource === null || typeof agentSource.ui_contract_snapshot_json !== "function") {
            hydrated = true
            safeMessage = qsTr("This build does not expose the required frontend compatibility contract.")
            negotiationCompleted(false)
            return false
        }
        try {
            var value = JSON.parse(agentSource.ui_contract_snapshot_json())
            bridgeContractVersion = Number(value.contract_version || 0)
            minimumQmlVersion = Number(value.min_qml_contract_version || 0)
            maximumQmlVersion = Number(value.max_qml_contract_version || 0)
            commandSchemaVersion = Number(value.command_schema_version || 0)
            eventSchemaVersion = Number(value.event_schema_version || 0)
            snapshotSchemaVersion = Number(value.snapshot_schema_version || 0)
            requiredFeatures = Array.isArray(value.required_features) ? value.required_features : []

            var protocol = value.protocol && typeof value.protocol === "object" ? value.protocol : ({})
            var currentVersion = protocol.current_version && typeof protocol.current_version === "object"
                    ? protocol.current_version : ({})
            bridgeProtocolMajor = Number(currentVersion.major || 0)
            bridgeProtocolMinor = Number(currentVersion.minor || 0)
            minimumPeerProtocolMajor = Number(protocol.minimum_supported_peer_major || 0)
            protocolCapabilities = Array.isArray(protocol.capabilities) ? protocol.capabilities : []

            var featuresPresent = containsAll(supportedFeatures, requiredFeatures)
                    && containsAll(requiredFeatures, [
                        "typed_commands", "typed_events", "snapshot_delta_sync",
                        "persistent_ui_session", "capability_gating",
                        "command_envelope_v2", "command_acknowledgement"
                    ])
            var commandProtocolPresent = containsAll(protocolCapabilities, [
                "command_envelope_v2", "command_acknowledgement",
                "scoped_chat_operations"
            ])
            var reliableV2Events = eventSchemaVersion === 2
                    && containsAll(protocolCapabilities, [
                        "event_identity", "per_stream_sequencing",
                        "operation_lifecycle_events"
                    ])
            var isolatedLegacyEvents = eventSchemaVersion === 1
                    && protocolCapabilities.indexOf("legacy_event_v1_compatibility") >= 0
            compatible = Number(value.schema_version || 0) === 1
                    && qmlContractVersion >= minimumQmlVersion
                    && qmlContractVersion <= maximumQmlVersion
                    && commandSchemaVersion === 2
                    && snapshotSchemaVersion === 1
                    && bridgeProtocolMajor === protocolMajor
                    && minimumPeerProtocolMajor <= protocolMajor
                    && featuresPresent
                    && commandProtocolPresent
                    && (reliableV2Events || isolatedLegacyEvents)
            eventMode = compatible ? (reliableV2Events ? "protocol_v2" : "legacy_v1") : "unknown"
            recomputeProtocolMode()
            safeMessage = compatible
                    ? ""
                    : qsTr("This frontend and local service use incompatible architecture or protocol contracts. Update them together before opening private data.")
        } catch (error) {
            compatible = false
            eventMode = "unknown"
            recomputeProtocolMode()
            safeMessage = qsTr("The local service returned an invalid compatibility contract.")
        }
        hydrated = true
        negotiationCompleted(compatible)
        return compatible
    }
}
