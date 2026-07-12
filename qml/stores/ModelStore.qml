pragma Singleton
import QtQuick
import QtQml.Models

QtObject {
    property var bridgeSource: null
    property var agentSource: null
    property var models: ListModel { id: modelList; dynamicRoles: true }
    property string selectedModelId: ""
    property string activeModelId: "" // compatibility alias: selected for next session
    property string loadedModelId: ""
    property string inferenceBackend: "unknown"
    property bool activationSupported: false
    property bool restartRequired: false
    property string sessionMessage: ""
    property bool hydrated: false
    readonly property int count: modelList.count
    readonly property bool hasInstalledModel: {
        for (var i = 0; i < modelList.count; ++i)
            if (modelList.get(i).installed === true)
                return true
        return false
    }

    signal hydrationCompleted
    signal modelChanged(string modelId)

    function configure(bridge, agent) {
        bridgeSource = bridge
        agentSource = agent
    }

    function formatBytes(bytes) {
        var value = Number(bytes || 0)
        if (value >= 1024 * 1024 * 1024)
            return (value / (1024 * 1024 * 1024)).toFixed(1) + " GB"
        if (value >= 1024 * 1024)
            return (value / (1024 * 1024)).toFixed(0) + " MB"
        return (value / 1024).toFixed(0) + " KB"
    }

    function findIndex(modelId) {
        for (var i = 0; i < modelList.count; ++i)
            if (modelList.get(i).modelId === modelId)
                return i
        return -1
    }

    function hydrate() {
        modelList.clear()
        if (bridgeSource !== null && typeof bridgeSource.model_catalogue_json === "function") {
            try {
                var rows = JSON.parse(bridgeSource.model_catalogue_json())
                if (Array.isArray(rows)) {
                    for (var i = 0; i < rows.length; ++i) {
                        var row = rows[i]
                        modelList.append({
                            modelId: row.id || "",
                            displayName: row.display_name || qsTr("Local model"),
                            description: row.description || "",
                            approximateBytes: Number(row.approximate_bytes || 0),
                            sizeLabel: formatBytes(row.approximate_bytes),
                            minRamMiB: Number(row.min_device_ram_mib || 0),
                            contextTokens: Number(row.recommended_n_ctx || 0),
                            filename: row.filename || "",
                            installed: row.installed === true,
                            bytesOnDisk: Number(row.bytes_on_disk || 0),
                            downloadState: "idle",
                            progress: 0,
                            errorCode: ""
                        })
                    }
                }
            } catch (error) {
                ErrorStore.push({
                    code: "ERR_UI_MODEL_SNAPSHOT",
                    severity: "warning",
                    recoverable: true,
                    safe_message: qsTr("The local model catalogue could not be loaded.")
                })
            }
        }
        selectedModelId = UiSessionStore.selectedModelId
        if (!selectedModelId && modelList.count > 0)
            selectedModelId = modelList.get(0).modelId
        activeModelId = selectedModelId
        loadedModelId = ""
        inferenceBackend = "unknown"
        activationSupported = false
        restartRequired = selectedModelId.length > 0
        sessionMessage = ""
        if (agentSource !== null && typeof agentSource.engine_session_snapshot_json === "function") {
            try {
                var session = JSON.parse(agentSource.engine_session_snapshot_json())
                loadedModelId = session.loaded_model_id || ""
                inferenceBackend = session.inference_backend || "unknown"
                activationSupported = session.activation_supported === true
                restartRequired = session.restart_required === true
                sessionMessage = session.safe_message || ""
            } catch (error) {
                ErrorStore.push({
                    code: "ERR_UI_ENGINE_SESSION", severity: "warning", recoverable: true,
                    safe_message: qsTr("The model session state could not be restored.")
                })
            }
        }
        hydrated = true
        hydrationCompleted()
    }

    function selectModel(modelId) {
        if (findIndex(modelId) < 0)
            return false
        selectedModelId = modelId
        UiSessionStore.setSelectedModel(modelId)
        modelChanged(modelId)
        return true
    }


    function parseResult(raw, fallbackCode) {
        try {
            var value = JSON.parse(raw)
            if (!value || value.ok !== true) {
                ErrorStore.push(value && value.error ? value.error : ({
                    code: fallbackCode, severity: "warning", recoverable: true,
                    safe_message: qsTr("The model operation could not be completed.")
                }), fallbackCode)
                return null
            }
            return value
        } catch (error) {
            ErrorStore.push({ code: fallbackCode, severity: "warning", recoverable: true,
                              safe_message: qsTr("The model operation returned an invalid response.") }, fallbackCode)
            return null
        }
    }

    function selectInstalledModel(modelId) {
        var index = findIndex(modelId)
        if (index < 0 || modelList.get(index).installed !== true)
            return false
        if (agentSource === null || typeof agentSource.select_installed_model_json !== "function")
            return false
        var value = parseResult(agentSource.select_installed_model_json(modelId), "ERR_UI_MODEL_SELECT")
        if (!value)
            return false
        activeModelId = modelId
        loadedModelId = ""
        restartRequired = true
        sessionMessage = value.message || qsTr("The model is selected for the next engine session.")
        selectModel(modelId)
        return true
    }

    function deleteInstalledModel(modelId) {
        var index = findIndex(modelId)
        if (index < 0 || modelList.get(index).installed !== true)
            return false
        if (agentSource === null || typeof agentSource.delete_installed_model_json !== "function")
            return false
        var value = parseResult(agentSource.delete_installed_model_json(modelId), "ERR_UI_MODEL_DELETE")
        if (!value)
            return false
        modelList.setProperty(index, "installed", false)
        modelList.setProperty(index, "bytesOnDisk", 0)
        modelList.setProperty(index, "downloadState", "idle")
        if (activeModelId === modelId)
            activeModelId = ""
        if (selectedModelId === modelId) {
            selectedModelId = ""
            UiSessionStore.setSelectedModel("")
        }
        StorageStore.scheduleRefresh()
        modelChanged(modelId)
        return true
    }

    function updateDownload(modelId, state, progress, errorCode) {
        var index = findIndex(modelId)
        if (index < 0)
            return
        if (state)
            modelList.setProperty(index, "downloadState", state)
        if (typeof progress === "number")
            modelList.setProperty(index, "progress", Math.max(0, Math.min(1, progress)))
        if (errorCode !== undefined)
            modelList.setProperty(index, "errorCode", errorCode || "")
        if (state === "completed") {
            modelList.setProperty(index, "installed", true)
            modelList.setProperty(index, "progress", 1)
        }
        modelChanged(modelId)
    }

    function applyEvent(event) {
        if (!event)
            return
        if (event.category === "download_state")
            updateDownload(event.model_id || "", event.state || "idle", undefined,
                           event.error ? event.error.code : "")
        else if (event.category === "download_progress")
            updateDownload(event.model_id || "", event.state || "downloading",
                           Number(event.progress || 0), "")
        else if (event.category === "download_completed") {
            updateDownload(event.model_id || "", "completed", 1, "")
            StorageStore.scheduleRefresh()
        }
    }
}
