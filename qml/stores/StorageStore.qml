pragma Singleton
import QtQuick

QtObject {
    id: root
    property var agentSource: null
    property real modelBytes: 0
    property real partialBytes: 0
    property real totalBytes: 0
    property real accountedModelBytes: 0
    property real maxModelStorageBytes: 1
    property real usageRatio: 0
    property string pressure: "normal"
    property bool hydrated: false
    property bool loading: false
    property string lastRequestId: ""
    readonly property bool warning: pressure === "warning" || pressure === "critical"
    readonly property bool critical: pressure === "critical"

    signal hydrationCompleted

    property Timer refreshTimer: Timer {
        interval: 350
        repeat: false
        onTriggered: root.hydrate()
    }

    function configure(agent) { agentSource = agent }
    function scheduleRefresh() { refreshTimer.restart() }

    function applySnapshot(value) {
        if (value) {
            modelBytes = Number(value.model_bytes || 0)
            partialBytes = Number(value.partial_bytes || 0)
            totalBytes = Number(value.total_bytes || 0)
            accountedModelBytes = Number(value.accounted_model_bytes || 0)
            maxModelStorageBytes = Math.max(1, Number(value.max_model_storage_bytes || 1))
            usageRatio = Math.max(0, Math.min(1, Number(value.usage_ratio || 0)))
            pressure = value.pressure || "normal"
        }
        loading = false
        hydrated = true
        hydrationCompleted()
    }

    function hydrate() {
        if (loading)
            return
        hydrated = false
        if (agentSource === null || typeof agentSource.storage_snapshot_json !== "function") {
            applySnapshot(null)
            return
        }
        loading = true
        try {
            var value = JSON.parse(agentSource.storage_snapshot_json())
            if (value && value.accepted === true) {
                lastRequestId = value.request_id || ""
                return
            }
            if (value && value.error)
                ErrorStore.push(value.error, "ERR_UI_STORAGE_SNAPSHOT")
            else
                applySnapshot(value)
        } catch (error) {
            loading = false
            hydrated = true
            ErrorStore.push({ code: "ERR_UI_STORAGE_SNAPSHOT", severity: "warning", recoverable: true,
                              safe_message: qsTr("Storage usage could not be measured.") })
            hydrationCompleted()
        }
    }

    function formatBytes(bytes) {
        var value = Number(bytes || 0)
        if (value >= 1024 * 1024 * 1024)
            return (value / (1024 * 1024 * 1024)).toFixed(1) + " GB"
        if (value >= 1024 * 1024)
            return (value / (1024 * 1024)).toFixed(0) + " MB"
        return (value / 1024).toFixed(0) + " KB"
    }

    property Connections asyncResultConnections: Connections {
        target: root.agentSource
        ignoreUnknownSignals: true
        function onAsync_result(resultJson) {
            var result
            try { result = JSON.parse(resultJson) } catch (error) { return }
            if (!result || result.domain !== "storage.snapshot"
                    || result.request_id !== root.lastRequestId || result.current === false)
                return
            if (result.ok === true)
                root.applySnapshot(result.payload)
            else {
                root.loading = false
                root.hydrated = true
                ErrorStore.push(result.error, "ERR_UI_STORAGE_SNAPSHOT")
                root.hydrationCompleted()
            }
        }
    }
}
