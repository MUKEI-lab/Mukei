pragma Singleton
import QtQuick
import QtQml.Models

QtObject {
    id: root
    property var agentSource: null
    property var jobs: ListModel { id: jobList; dynamicRoles: true }
    property bool hydrated: false
    property bool loading: false
    property string lastRequestId: ""
    readonly property int count: jobList.count
    readonly property int activeCount: {
        var total = 0
        for (var i = 0; i < jobList.count; ++i) {
            var state = jobList.get(i).state
            if (["queued", "starting", "downloading", "cancelling"].indexOf(state) >= 0)
                total++
        }
        return total
    }

    signal hydrationCompleted
    signal snapshotApplied
    signal snapshotFailed

    function configure(agent) { agentSource = agent }

    function findIndex(jobId, modelId) {
        for (var i = 0; i < jobList.count; ++i) {
            var row = jobList.get(i)
            if ((jobId && row.jobId === jobId) || (!jobId && modelId && row.modelId === modelId))
                return i
        }
        return -1
    }

    function normalize(row) {
        var expected = Number(row.expected_bytes || row.expectedBytes || 0)
        var downloaded = Number(row.bytes_downloaded || row.bytesDownloaded || 0)
        return {
            jobId: row.job_id || row.jobId || "",
            modelId: row.model_id || row.modelId || "",
            destinationToken: row.destination_token || row.destinationToken || "",
            expectedBytes: expected,
            bytesDownloaded: downloaded,
            progress: expected > 0 ? Math.max(0, Math.min(1, downloaded / expected)) : 0,
            state: row.status || row.state || "queued",
            lastErrorCode: row.last_error_code || row.lastErrorCode || "",
            createdAt: row.created_at || row.createdAt || "",
            updatedAt: row.updated_at || row.updatedAt || ""
        }
    }

    function upsert(row) {
        var normalized = normalize(row)
        var index = findIndex(normalized.jobId, normalized.modelId)
        if (index < 0)
            jobList.insert(0, normalized)
        else
            jobList.set(index, normalized)
    }

    function applySnapshot(value) {
        if (!Array.isArray(value))
            value = []
        jobList.clear()
        for (var i = 0; i < value.length; ++i)
            jobList.append(normalize(value[i]))
        loading = false
        hydrated = true
        OperationStore.reconcileDurableState()
        snapshotApplied()
        hydrationCompleted()
    }

    function failSnapshot(errorValue) {
        loading = false
        hydrated = true
        if (errorValue)
            ErrorStore.push(errorValue, "ERR_UI_DOWNLOAD_SNAPSHOT")
        snapshotFailed()
        hydrationCompleted()
    }

    function hydrate() {
        if (loading)
            return false
        hydrated = false
        if (agentSource === null || typeof agentSource.download_jobs_json !== "function") {
            applySnapshot([])
            return true
        }
        loading = true
        try {
            var value = JSON.parse(agentSource.download_jobs_json(100))
            if (value && value.accepted === true) {
                lastRequestId = value.request_id || ""
                return true
            }
            if (value && value.error) {
                failSnapshot(value.error)
                return false
            }
            applySnapshot(value)
            return true
        } catch (error) {
            failSnapshot({
                code: "ERR_UI_DOWNLOAD_SNAPSHOT",
                severity: "warning",
                recoverable: true,
                safe_message: qsTr("Download history could not be restored.")
            })
            return false
        }
    }

    function applyEvent(event) {
        if (!event || ["download_state", "download_progress", "download_completed"].indexOf(event.category) < 0)
            return
        var modelId = event.model_id || ""
        var index = findIndex("", modelId)
        var existing = index >= 0 ? jobList.get(index) : ({})
        var expected = Number(event.total_bytes || existing.expectedBytes || 0)
        var downloaded = Number(event.bytes_downloaded || existing.bytesDownloaded || 0)
        var state = event.category === "download_completed" ? "completed" : (event.state || "downloading")
        upsert({
            jobId: existing.jobId || "event:" + (modelId || "active"), modelId: modelId,
            destinationToken: event.destination || existing.destinationToken || "",
            expectedBytes: expected, bytesDownloaded: downloaded, state: state,
            lastErrorCode: event.error ? event.error.code : (existing.lastErrorCode || ""),
            createdAt: existing.createdAt || event.timestamp || "", updatedAt: event.timestamp || ""
        })
        if (["completed", "failed", "cancelled"].indexOf(state) >= 0)
            Qt.callLater(hydrate)
    }

    property var agentConnections: Connections {
        target: root.agentSource
        ignoreUnknownSignals: true
        function onAsync_result(resultJson) {
            var result
            try { result = JSON.parse(resultJson) } catch (error) { return }
            if (!result || result.domain !== "downloads.snapshot"
                    || result.request_id !== root.lastRequestId || result.current === false)
                return
            if (result.ok === true)
                root.applySnapshot(result.payload)
            else
                root.failSnapshot(result.error)
        }
    }
}
