from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{path}: expected one Android M1 anchor, found {count}')
    file.write_text(text.replace(old, new, 1))

path = 'android/app/src/main/kotlin/ai/mukei/android/BackendRuntimeHost.kt'
replace_once(
    path,
    '''    data class ProjectCommandSubmission(
''',
    '''    fun submitUniversalStorageImport(
        parentNodeId: String,
        target: String,
        displayName: String,
        mimeType: String,
    ): StorageImportSubmission {
        val activeGateway = gateway.get()
            ?: return StorageImportSubmission("rejected", null, "backend_unavailable")
        if (parentNodeId.isBlank() || target.isBlank() || displayName.isBlank() || mimeType.isBlank()) {
            return StorageImportSubmission("rejected", null, "invalid_payload")
        }
        return try {
            val envelope = JSONObject()
                .put("protocol_version", JSONObject().put("major", 2).put("minor", 4))
                .put("command_id", UUID.randomUUID().toString())
                .put("request_id", UUID.randomUUID().toString())
                .put("command_type", "storage.import_file")
                .put("submitted_at", Instant.now().toString())
                .put("correlation_id", UUID.randomUUID().toString())
                .put("idempotency_key", "storage-import-${UUID.randomUUID()}")
                .put("scope", JSONObject())
                .put(
                    "payload",
                    JSONObject()
                        .put("target", target)
                        .put("display_name", displayName)
                        .put("mime_type", mimeType)
                        .put("parent_node_id", parentNodeId),
                )
            storageSubmission(activeGateway, envelope)
        } catch (failure: Throwable) {
            StorageImportSubmission("rejected", null, stableFailureCode(failure))
        }
    }

    fun createStorageDirectory(parentNodeId: String, name: String): StorageImportSubmission =
        submitStorageWorkspaceCommand(
            commandType = "storage.directory.create",
            payload = JSONObject().put("parent_node_id", parentNodeId).put("name", name),
        )

    fun renameStorageNode(nodeId: String, name: String): StorageImportSubmission =
        submitStorageWorkspaceCommand(
            commandType = "storage.node.rename",
            payload = JSONObject().put("node_id", nodeId).put("name", name),
        )

    fun trashStorageNode(nodeId: String): StorageImportSubmission = submitStorageWorkspaceCommand(
        commandType = "storage.node.trash",
        payload = JSONObject().put("node_id", nodeId),
    )

    fun restoreStorageNode(nodeId: String): StorageImportSubmission = submitStorageWorkspaceCommand(
        commandType = "storage.node.restore",
        payload = JSONObject().put("node_id", nodeId),
    )

    private fun submitStorageWorkspaceCommand(
        commandType: String,
        payload: JSONObject,
    ): StorageImportSubmission {
        val activeGateway = gateway.get()
            ?: return StorageImportSubmission("rejected", null, "backend_unavailable")
        return try {
            val envelope = JSONObject()
                .put("protocol_version", JSONObject().put("major", 2).put("minor", 4))
                .put("command_id", UUID.randomUUID().toString())
                .put("request_id", UUID.randomUUID().toString())
                .put("command_type", commandType)
                .put("submitted_at", Instant.now().toString())
                .put("correlation_id", UUID.randomUUID().toString())
                .put("idempotency_key", "storage-workspace-${UUID.randomUUID()}")
                .put("payload", payload)
            storageSubmission(activeGateway, envelope)
        } catch (failure: Throwable) {
            StorageImportSubmission("rejected", null, stableFailureCode(failure))
        }
    }

    private fun storageSubmission(
        activeGateway: RustNativeGateway,
        envelope: JSONObject,
    ): StorageImportSubmission {
        val acknowledgement = JSONObject(
            String(
                activeGateway.submitCommand(
                    envelope.toString().toByteArray(StandardCharsets.UTF_8),
                ),
                StandardCharsets.UTF_8,
            ),
        )
        return StorageImportSubmission(
            status = acknowledgement.optString("status", "rejected"),
            operationId = acknowledgement.optString("operation_id").takeIf { it.isNotBlank() },
            rejectionReason = acknowledgement.optString("rejection_reason").takeIf { it.isNotBlank() },
        )
    }

    data class ProjectCommandSubmission(
''',
)
replace_once(
    path,
    '''        if (nativeDomain !in setOf("application", "settings", "protocol", "operations", "projects")) {
''',
    '''        if (nativeDomain !in setOf("application", "settings", "protocol", "operations", "projects", "storage")) {
''',
)

print('storage workspace M1 Android bridge patch applied')
