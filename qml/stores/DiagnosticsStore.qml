pragma Singleton
import QtQuick

QtObject {
    property var agentSource: null
    property var snapshot: ({})
    property bool hydrated: false
    property bool exporting: false
    property string lastExportId: ""
    property string lastExportFilename: ""

    // Derived presentation state only. Rust remains authoritative for privacy
    // policy, privacy epoch and queue accounting.
    readonly property string diagnosticsMode: policyMode(snapshot)
    readonly property bool localDiagnosticsEnabled: diagnosticsMode !== "disabled"
    readonly property bool exportAllowed: policyExportAllowed(snapshot)
    readonly property bool queuePressureDegraded: pressureDegraded(snapshot)
    readonly property int droppedCount: totalDropped(snapshot)
    readonly property int coalescedCount: totalCoalesced(snapshot)
    readonly property int privacyEpoch: snapshot && snapshot.privacy_epoch !== undefined
                                        ? Number(snapshot.privacy_epoch) : 0

    signal hydrationCompleted
    signal exportCompleted(string exportId, string filename)

    function configure(agent) { agentSource = agent }

    function policyMode(value) {
        if (!value || !value.policy)
            return "disabled"
        var mode = value.policy.mode !== undefined ? value.policy.mode : value.policy
        return String(mode || "disabled").toLowerCase()
    }

    function policyExportAllowed(value) {
        if (!value || !value.policy)
            return false
        return value.policy.export_allowed === true || value.policy.exportAllowed === true
    }

    function recorderStats(value) {
        return value && value.recorder_stats ? value.recorder_stats : ({})
    }

    function pressureDegraded(value) {
        var stats = recorderStats(value)
        var sinkHealth = String(stats.sink_health || "").toLowerCase()
        return sinkHealth === "degraded"
                || sinkHealth === "disconnected"
                || Number(stats.sink_queue_drops || 0) > 0
                || Number(stats.sink_disconnected_drops || 0) > 0
                || Number(stats.sink_oversized_drops || 0) > 0
                || Number(stats.sink_slow_callbacks || 0) > 0
    }

    function totalDropped(value) {
        var stats = recorderStats(value)
        return Number(stats.events_dropped_policy || 0)
                + Number(stats.event_oversized_drops || 0)
                + Number(stats.event_capacity_drops || 0)
                + Number(stats.metric_observations_dropped_policy || 0)
                + Number(stats.health_signals_dropped_policy || 0)
                + Number(stats.slo_observations_dropped_policy || 0)
                + Number(stats.sink_queue_drops || 0)
                + Number(stats.sink_disconnected_drops || 0)
                + Number(stats.sink_oversized_drops || 0)
                + Number(stats.sink_privacy_epoch_drops || 0)
    }

    function totalCoalesced(value) {
        var stats = recorderStats(value)
        return Number(stats.health_signals_coalesced || 0)
                + Number(stats.sink_coalesced || 0)
    }

    function pushError(value, fallback) {
        ErrorStore.push(value && value.error ? value.error : ({
            code: fallback,
            severity: "warning",
            recoverable: true,
            safe_message: qsTr("The diagnostics request could not be completed.")
        }), fallback)
    }

    function hydrate() {
        if (agentSource === null || typeof agentSource.diagnostics_snapshot_json !== "function") {
            snapshot = ({})
            hydrated = true
            hydrationCompleted()
            return
        }
        try {
            var value = JSON.parse(agentSource.diagnostics_snapshot_json())
            if (value && value.error)
                pushError(value, "ERR_UI_DIAGNOSTICS_SNAPSHOT")
            else
                snapshot = value || ({})
        } catch (error) {
            pushError(null, "ERR_UI_DIAGNOSTICS_SNAPSHOT")
        }
        hydrated = true
        hydrationCompleted()
    }

    function exportBundle() {
        if (exporting || agentSource === null || typeof agentSource.export_diagnostics_json !== "function")
            return false
        exporting = true
        try {
            var value = JSON.parse(agentSource.export_diagnostics_json())
            if (!value || value.ok !== true) {
                pushError(value, "ERR_UI_DIAGNOSTICS_EXPORT")
                return false
            }
            lastExportId = value.export_id || ""
            lastExportFilename = value.filename || qsTr("diagnostics.json")
            exportCompleted(lastExportId, lastExportFilename)
            ErrorStore.push({
                code: "INFO_DIAGNOSTICS_EXPORTED",
                severity: "info",
                recoverable: false,
                safe_message: qsTr("A privacy-safe diagnostics report was created as %1.").arg(lastExportFilename)
            }, "INFO_DIAGNOSTICS_EXPORTED")
            return true
        } catch (error) {
            pushError(null, "ERR_UI_DIAGNOSTICS_EXPORT")
            return false
        } finally {
            exporting = false
        }
    }
}
