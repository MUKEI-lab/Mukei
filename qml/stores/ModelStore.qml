pragma Singleton
import QtQuick
import QtQml.Models

QtObject {
    id: root

    property var bridgeSource: null
    property var agentSource: null
    property var models: ListModel { id: modelList; dynamicRoles: true }
    property string selectedModelId: ""
    // Authoritative truth: this is the model whose backend is actually serving.
    property string activeModelId: ""
    property string loadedModelId: ""
    property string inferenceBackend: ""
    property string backendKind: "unavailable"
    property string backendUnavailableReason: ""
    property bool activationSupported: false
    property bool activationRequired: false
    property bool activationInProgress: false
    property bool activationFailed: false
    property string activationRequestId: ""
    property string activationOperationId: ""
    property string activationModelId: ""
    property bool activeModelReady: false
    property bool productReady: false
    property bool restartRequired: false
    property string safeMessage: ""
    // Compatibility surface used by the existing model manager.
    property string sessionMessage: safeMessage
    property bool hydrated: false

    readonly property int count: modelList.count
    readonly property bool hasInstalledModel: {
        for (var i = 0; i < modelList.count; ++i) {
            if (modelList.get(i).installed === true)
                return true
        }
        return false
    }
    readonly property int installedCount: {
        var total = 0
        for (var i = 0; i < modelList.count; ++i) {
            if (modelList.get(i).installed === true)
                total++
        }
        return total
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
        for (var i = 0; i < modelList.count; ++i) {
            if (modelList.get(i).modelId === modelId)
                return i
        }
        return -1
    }

    function normalize(row) {
        var approximateBytes = Number(row.approximate_bytes || row.approximateBytes || 0)
        return {
            modelId: row.id || row.model_id || row.modelId || "",
            displayName: row.display_name || row.displayName || row.id || qsTr("Local model"),
            description: row.description || "",
            approximateBytes: approximateBytes,
            sizeLabel: formatBytes(approximateBytes),
            minRamMiB: Number(row.min_device_ram_mib || row.minRamMiB || 0),
            contextTokens: Number(row.recommended_n_ctx || row.contextTokens || 0),
            filename: row.filename || "",
            installed: row.installed === true,
            bytesOnDisk: Number(row.bytes_on_disk || row.bytesOnDisk || 0),
            downloadState: row.downloadState || "idle",
            progress: Number(row.progress || 0),
            errorCode: row.errorCode || ""
        }
    }

    // Preserve the established public selection API for download/picker flows.
    // Selection is only candidate preference; it never claims backend readiness.
    function selectModel(modelId) {
        if (findIndex(modelId) < 0)
            return false
        selectedModelId = modelId
        UiSessionStore.setSelectedModel(modelId)
        modelChanged(modelId)
        return true
    }

    function beginActivation(modelId, requestId, operationId) {
        if (!modelId || !selectModel(modelId))
            return false
        activationModelId = modelId
        activationRequestId = requestId || ""
        activationOperationId = operationId || ""
        activationInProgress = true
        activationFailed = false
        activationRequired = activeModelId !== modelId
        restartRequired = false
        safeMessage = qsTr("Verifying and activating the selected model…")
        return true
    }

    function applyEngineSnapshot(session) {
        if (!session)
            return
        if (session.selected_model_id)
            selectedModelId = session.selected_model_id
        loadedModelId = session.loaded_model_id || ""
        activeModelId = loadedModelId
        inferenceBackend = session.inference_backend || ""
        backendKind = session.backend_kind || "unavailable"
        backendUnavailableReason = session.backend_unavailable_reason || ""
        activationSupported = session.activation_supported === true
        activationRequired = session.activation_required === true
        activationInProgress = session.activation_in_progress === true
        activationFailed = session.activation_failed === true
        activeModelReady = session.active_model_ready === true
        productReady = session.product_ready === true
        restartRequired = session.restart_required === true
        safeMessage = session.safe_message || ""
        if (!activationInProgress) {
            activationRequestId = ""
            activationOperationId = ""
            activationModelId = ""
        } else if (!activationModelId) {
            activationModelId = selectedModelId
        }
    }

    function refreshEngineSession() {
        if (agentSource === null || typeof agentSource.engine_session_snapshot_json !== "function")
            return false
        try {
            applyEngineSnapshot(JSON.parse(agentSource.engine_session_snapshot_json()))
            return true
        } catch (error) {
            ErrorStore.push({
                code: "ERR_UI_ENGINE_SESSION", severity: "warning", recoverable: true,
                safe_message: qsTr("The model session state could not be restored.")
            }, "ERR_UI_ENGINE_SESSION")
            return false
        }
    }

    function hydrate() {
        modelList.clear()
        if (bridgeSource !== null && typeof bridgeSource.model_catalogue_json === "function") {
            try {
                var catalogue = JSON.parse(bridgeSource.model_catalogue_json())
                if (Array.isArray(catalogue)) {
                    for (var i = 0; i < catalogue.length; ++i)
                        modelList.append(normalize(catalogue[i]))
                }
            } catch (error) {
                ErrorStore.push({
                    code: "ERR_UI_MODEL_SNAPSHOT", severity: "warning", recoverable: true,
                    safe_message: qsTr("The local model catalogue could not be loaded.")
                }, "ERR_UI_MODEL_SNAPSHOT")
            }
        }
        selectedModelId = UiSessionStore.selectedModelId
        if (!selectedModelId && modelList.count > 0)
            selectedModelId = modelList.get(0).modelId
        activeModelId = ""
        loadedModelId = ""
        inferenceBackend = ""
        backendKind = "unavailable"
        backendUnavailableReason = ""
        activationSupported = false
        activationRequired = false
        activationInProgress = false
        activationFailed = false
        activeModelReady = false
        productReady = false
        restartRequired = false
        safeMessage = ""
        refreshEngineSession()
        hydrated = true
        hydrationCompleted()
    }

    function selectInstalledModel(modelId) {
        var index = findIndex(modelId)
        if (index < 0 || modelList.get(index).installed !== true)
            return false
        if (agentSource === null || typeof agentSource.select_installed_model_json !== "function")
            return false
        try {
            var result = JSON.parse(agentSource.select_installed_model_json(modelId))
            if (result && result.accepted === true)
                return beginActivation(modelId, result.request_id || "", "")
            if (result && result.error)
                ErrorStore.push(result.error, "ERR_UI_MODEL_SELECT")
        } catch (error) {
            ErrorStore.push({
                code: "ERR_UI_MODEL_SELECT", severity: "warning", recoverable: true,
                safe_message: qsTr("The model could not be activated.")
            }, "ERR_UI_MODEL_SELECT")
        }
        return false
    }

    // Compatibility entry point retained for non-protocol callers. Production UI
    // uses IntentDispatcher, but existing tests/tools still depend on this method.
    function deleteInstalledModel(modelId) {
        var index = findIndex(modelId)
        if (index < 0 || modelList.get(index).installed !== true)
            return false
        if (agentSource === null || typeof agentSource.delete_installed_model_json !== "function")
            return false
        try {
            var value = JSON.parse(agentSource.delete_installed_model_json(modelId))
            if (!value || value.ok !== true) {
                ErrorStore.push(value && value.error ? value.error : ({
                    code: "ERR_UI_MODEL_DELETE", severity: "warning", recoverable: true,
                    safe_message: qsTr("The model could not be deleted.")
                }), "ERR_UI_MODEL_DELETE")
                return false
            }
        } catch (error) {
            ErrorStore.push({
                code: "ERR_UI_MODEL_DELETE", severity: "warning", recoverable: true,
                safe_message: qsTr("The model operation returned an invalid response.")
            }, "ERR_UI_MODEL_DELETE")
            return false
        }
        modelList.setProperty(index, "installed", false)
        modelList.setProperty(index, "bytesOnDisk", 0)
        modelList.setProperty(index, "downloadState", "idle")
        if (selectedModelId === modelId) {
            selectedModelId = ""
            UiSessionStore.setSelectedModel("")
        }
        StorageStore.scheduleRefresh()
        modelChanged(modelId)
        return true
    }

    function applyActivationSuccess(payload) {
        payload = payload || ({})
        if (activationModelId && payload.model_id && payload.model_id !== activationModelId)
            return false
        activeModelId = payload.active_model_id || payload.model_id || activeModelId
        loadedModelId = activeModelId
        inferenceBackend = payload.inference_backend || inferenceBackend
        backendKind = payload.backend_kind || backendKind
        activeModelReady = payload.active_model_ready === true
        productReady = payload.product_ready === true
        activationRequired = selectedModelId !== activeModelId
        activationFailed = false
        safeMessage = activeModelReady
                ? qsTr("The selected model is active and ready for inference.")
                : qsTr("The model activation completed without a ready backend.")
        modelChanged(activeModelId)
        return true
    }

    function applyAsyncResult(result) {
        if (!result || result.domain !== "model.activate"
                || result.request_id !== activationRequestId || result.current === false)
            return
        if (result.ok === true) {
            if (!applyActivationSuccess(result.payload))
                return
        } else {
            activationFailed = true
            if (result.error)
                ErrorStore.push(result.error, "ERR_UI_MODEL_ACTIVATION")
        }
        activationInProgress = false
        activationRequestId = ""
        activationOperationId = ""
        activationModelId = ""
        Qt.callLater(refreshEngineSession)
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
        if (event.category === "operation_lifecycle" && event.command_type === "model.select") {
            if (activationOperationId && event.operation_id !== activationOperationId)
                return
            if (event.state === "running") {
                var runningResult = event.result || ({})
                if (activationModelId && runningResult.model_id
                        && runningResult.model_id !== activationModelId)
                    return
                activationInProgress = true
                return
            }
            if (["completed", "failed", "cancelled"].indexOf(event.state) >= 0) {
                if (event.state === "completed") {
                    if (!applyActivationSuccess(event.result || ({})))
                        return
                } else if (event.state === "failed") {
                    activationFailed = true
                    if (event.error)
                        ErrorStore.push(event.error, "ERR_UI_MODEL_ACTIVATION")
                }
                activationInProgress = false
                activationRequestId = ""
                activationOperationId = ""
                activationModelId = ""
                Qt.callLater(refreshEngineSession)
                return
            }
        }
        if (event.category === "download_state") {
            updateDownload(event.model_id || "", event.state || "idle", undefined,
                           event.error ? event.error.code : "")
        } else if (event.category === "download_progress") {
            updateDownload(event.model_id || "", event.state || "downloading",
                           Number(event.progress || 0), "")
        } else if (event.category === "download_completed") {
            updateDownload(event.model_id || "", "completed", 1, "")
            StorageStore.scheduleRefresh()
        } else if (event.category === "download_failed") {
            updateDownload(event.model_id || "", "failed", undefined,
                           event.error ? event.error.code : "ERR_DOWNLOAD")
        }
    }

    property var agentConnections: Connections {
        target: root.agentSource
        ignoreUnknownSignals: true
        function onAsync_result(resultJson) {
            var result
            try { result = JSON.parse(resultJson) } catch (error) { return }
            root.applyAsyncResult(result)
        }
    }
}
