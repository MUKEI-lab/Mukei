pragma Singleton
import QtQuick
import QtQml.Models

QtObject {
    id: root
    property var agentSource: null
    property var documents: ListModel { id: documentList; dynamicRoles: true }
    property bool hydrated: false
    property bool loading: false
    property string snapshotRequestId: ""
    property var pendingOperations: ({})
    readonly property int count: documentList.count
    readonly property int cleanupPendingCount: {
        var total = 0
        for (var i = 0; i < documentList.count; ++i)
            if (documentList.get(i).cleanupPending === true)
                total++
        return total
    }

    signal hydrationCompleted

    function configure(agent) { agentSource = agent }

    function registerAccepted(raw, expectedDomain, fallbackCode) {
        try {
            var value = JSON.parse(raw)
            if (value && value.accepted === true && value.domain === expectedDomain) {
                var next = Object.assign({}, pendingOperations)
                next[value.request_id || ""] = expectedDomain
                pendingOperations = next
                return true
            }
            if (value && value.ok === true) {
                if (expectedDomain === "documents.grant") {
                    ErrorStore.push({
                        code: "INFO_DOCUMENT_ACCESS_GRANTED", severity: "info", recoverable: false,
                        safe_message: value.indexed === true
                                      ? qsTr("The private document is ready.")
                                      : qsTr("Private access was granted. Indexing will be available when the ingestion pipeline is connected.")
                    }, "INFO_DOCUMENT_ACCESS_GRANTED")
                } else if (expectedDomain === "documents.retry") {
                    ErrorStore.push({
                        code: "INFO_DOCUMENT_INGESTION_QUEUED", severity: "info", recoverable: false,
                        safe_message: qsTr("Document indexing was queued. It will start when the on-device embedder is available.")
                    }, "INFO_DOCUMENT_INGESTION_QUEUED")
                }
                Qt.callLater(hydrate)
                return true
            }
            ErrorStore.push(value && value.error ? value.error : ({
                code: fallbackCode, severity: "warning", recoverable: true,
                safe_message: qsTr("The private document operation could not be completed.")
            }), fallbackCode)
            return false
        } catch (error) {
            ErrorStore.push({ code: fallbackCode, severity: "warning", recoverable: true,
                              safe_message: qsTr("The private document operation returned an invalid response.") }, fallbackCode)
            return false
        }
    }

    function grantAccess(target, label, mimeType) {
        if (agentSource === null || typeof agentSource.grant_document_access_json !== "function")
            return false
        return registerAccepted(agentSource.grant_document_access_json(target, label, mimeType),
                                "documents.grant", "ERR_UI_DOCUMENT_GRANT")
    }

    function retryIngestion(documentId) {
        if (!documentId || agentSource === null
                || typeof agentSource.retry_document_ingestion_json !== "function")
            return false
        return registerAccepted(agentSource.retry_document_ingestion_json(documentId),
                                "documents.retry", "ERR_UI_DOCUMENT_INGESTION")
    }

    function revokeDocument(documentId) {
        if (!documentId || agentSource === null || typeof agentSource.revoke_document_json !== "function")
            return false
        return registerAccepted(agentSource.revoke_document_json(documentId),
                                "documents.revoke", "ERR_UI_DOCUMENT_REVOKE")
    }

    function appendDocument(row) {
        documentList.append({
            documentId: row.document_id || "",
            label: row.label || qsTr("Private document"),
            mimeType: row.mime_type || "",
            sizeBytes: Number(row.size_bytes || 0),
            chunkCount: Number(row.chunk_count || 0),
            revoked: row.revoked === true,
            cleanupPending: row.cleanup_pending === true,
            cleanupAttempts: Number(row.cleanup_attempts || 0),
            lastError: row.last_error || "",
            permissionState: row.permission_state || "unknown",
            ingestionState: row.ingestion_state || "waiting_for_embedder",
            ingestionProgress: Number(row.ingestion_progress_percent || 0),
            ingestionRetryable: row.ingestion_retryable !== false,
            ingestionError: row.ingestion_error || "",
            updatedAt: row.updated_at || ""
        })
    }

    function applySnapshot(value) {
        if (!Array.isArray(value))
            value = []
        documentList.clear()
        for (var i = 0; i < value.length; ++i)
            appendDocument(value[i])
        loading = false
        hydrated = true
        OperationStore.reconcileDurableState()
        hydrationCompleted()
    }

    function hydrate() {
        if (loading)
            return
        hydrated = false
        if (agentSource === null || typeof agentSource.document_list_json !== "function") {
            applySnapshot([])
            return
        }
        loading = true
        try {
            var value = JSON.parse(agentSource.document_list_json(250))
            if (value && value.accepted === true) {
                snapshotRequestId = value.request_id || ""
                return
            }
            if (value && value.error) {
                loading = false
                hydrated = true
                ErrorStore.push(value.error, "ERR_UI_DOCUMENT_SNAPSHOT")
                hydrationCompleted()
            } else {
                applySnapshot(value)
            }
        } catch (error) {
            loading = false
            hydrated = true
            ErrorStore.push({ code: "ERR_UI_DOCUMENT_SNAPSHOT", severity: "warning", recoverable: true,
                              safe_message: qsTr("Private document status could not be restored.") })
            hydrationCompleted()
        }
    }

    function finishOperation(result) {
        var next = Object.assign({}, pendingOperations)
        delete next[result.request_id || ""]
        pendingOperations = next
        if (result.ok !== true) {
            var code = result.domain === "documents.grant" ? "ERR_UI_DOCUMENT_GRANT"
                     : result.domain === "documents.revoke" ? "ERR_UI_DOCUMENT_REVOKE"
                     : "ERR_UI_DOCUMENT_INGESTION"
            ErrorStore.push(result.error, code)
            return
        }
        if (result.domain === "documents.grant") {
            ErrorStore.push({
                code: "INFO_DOCUMENT_ACCESS_GRANTED", severity: "info", recoverable: false,
                safe_message: result.payload && result.payload.indexed === true
                              ? qsTr("The private document is ready.")
                              : qsTr("Private access was granted. Indexing will be available when the ingestion pipeline is connected.")
            }, "INFO_DOCUMENT_ACCESS_GRANTED")
        } else if (result.domain === "documents.retry") {
            ErrorStore.push({
                code: "INFO_DOCUMENT_INGESTION_QUEUED", severity: "info", recoverable: false,
                safe_message: qsTr("Document indexing was queued. It will start when the on-device embedder is available.")
            }, "INFO_DOCUMENT_INGESTION_QUEUED")
        }
        Qt.callLater(hydrate)
    }

    property var agentConnections: Connections {
        target: root.agentSource
        ignoreUnknownSignals: true
        function onAsync_result(resultJson) {
            var result
            try { result = JSON.parse(resultJson) } catch (error) { return }
            if (!result)
                return
            if (result.domain === "documents.snapshot"
                    && result.request_id === root.snapshotRequestId
                    && result.current !== false) {
                if (result.ok === true)
                    root.applySnapshot(result.payload)
                else {
                    root.loading = false
                    root.hydrated = true
                    ErrorStore.push(result.error, "ERR_UI_DOCUMENT_SNAPSHOT")
                    root.hydrationCompleted()
                }
                return
            }
            if (result.domain === "documents.grant"
                    || result.domain === "documents.revoke"
                    || result.domain === "documents.retry") {
                if (root.pendingOperations[result.request_id || ""])
                    root.finishOperation(result)
            }
        }
    }
}
