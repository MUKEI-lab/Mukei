package ai.mukei.android

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import ai.mukei.android.designsystem.MukeiSpacing
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

private const val StorageNavigationPreferences = "mukei-storage-navigation"
private const val StorageCurrentNodeKey = "current-node-id"
private const val MaxStorageNameCharacters = 255

private data class StorageNodeCard(
    val nodeId: String,
    val parentNodeId: String?,
    val nodeType: String,
    val displayName: String,
    val state: String,
    val systemRole: String?,
    val sizeBytes: Long?,
    val mimeType: String?,
    val updatedAt: String,
)

private data class StorageWorkspaceSnapshot(
    val scopeId: String,
    val rootNodeId: String,
    val nodes: List<StorageNodeCard>,
)

@Composable
internal fun StorageSurface() {
    val context = LocalContext.current
    val navigationPreferences = remember(context) {
        context.getSharedPreferences(StorageNavigationPreferences, Context.MODE_PRIVATE)
    }
    val mainHandler = remember { Handler(Looper.getMainLooper()) }
    var snapshot by remember { mutableStateOf<StorageWorkspaceSnapshot?>(null) }
    var refreshGeneration by remember { mutableIntStateOf(0) }
    var currentNodeId by rememberSaveable {
        mutableStateOf(navigationPreferences.getString(StorageCurrentNodeKey, null))
    }
    var searchQuery by rememberSaveable { mutableStateOf("") }
    var banner by remember { mutableStateOf<String?>(null) }
    var createFolderOpen by remember { mutableStateOf(false) }
    var renameTarget by remember { mutableStateOf<StorageNodeCard?>(null) }

    LaunchedEffect(refreshGeneration) {
        val loaded = withContext(Dispatchers.IO) { loadStorageWorkspaceSnapshot() }
        snapshot = loaded
        if (loaded != null) {
            val candidate = currentNodeId
            val valid = candidate != null && loaded.nodes.any {
                it.nodeId == candidate && it.nodeType == "directory" && it.state != "deleted"
            }
            if (!valid) {
                currentNodeId = loaded.rootNodeId
                navigationPreferences.edit().putString(StorageCurrentNodeKey, loaded.rootNodeId).apply()
            }
        }
    }

    DisposableEffect(Unit) {
        val registration = BackendRuntimeHost.addEventListener { batch ->
            var storageChanged = false
            var failureCode: String? = null
            batch.events.forEach { raw ->
                runCatching {
                    val event = JSONObject(raw)
                    val eventType = event.optString("event_type")
                    val commandType = event.optString("command_type")
                    if (eventType.startsWith("storage.")) storageChanged = true
                    if (eventType == "operation.failed" && commandType.startsWith("storage.")) {
                        failureCode = event.optJSONObject("payload")?.optString("code")
                            ?.takeIf { it.isNotBlank() }
                        storageChanged = true
                    }
                }
            }
            if (storageChanged) {
                mainHandler.post {
                    refreshGeneration += 1
                    failureCode?.let { banner = storageFailureMessage(it) }
                }
            }
        }
        onDispose { registration.close() }
    }

    val loaded = snapshot
    val selectedNodeId = currentNodeId ?: loaded?.rootNodeId
    val currentNode = loaded?.nodes?.firstOrNull { it.nodeId == selectedNodeId }
    val insideTrash = loaded != null && selectedNodeId != null && isInsideTrash(loaded, selectedNodeId)
    val writableDirectory = currentNode != null &&
        currentNode.nodeType == "directory" &&
        currentNode.state == "active" &&
        currentNode.systemRole != "trash" &&
        !insideTrash

    fun openDirectory(nodeId: String) {
        val activeSnapshot = snapshot ?: return
        val target = activeSnapshot.nodes.firstOrNull {
            it.nodeId == nodeId && it.nodeType == "directory" && it.state != "deleted"
        } ?: return
        currentNodeId = target.nodeId
        searchQuery = ""
        navigationPreferences.edit().putString(StorageCurrentNodeKey, target.nodeId).apply()
    }

    val documentPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val parentNodeId = currentNodeId ?: snapshot?.rootNodeId
        if (uri == null || parentNodeId == null || !writableDirectory) return@rememberLauncherForActivityResult
        runCatching {
            context.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
        }
        val displayName = resolveDocumentDisplayName(context, uri)
        val mimeType = context.contentResolver.getType(uri)?.takeIf { it.isNotBlank() }
            ?: "application/octet-stream"
        val submission = BackendRuntimeHost.submitUniversalStorageImport(
            parentNodeId = parentNodeId,
            target = uri.toString(),
            displayName = displayName,
            mimeType = mimeType,
        )
        banner = if (submission.status == "accepted") {
            "Importing $displayName into encrypted storage…"
        } else {
            "Import rejected: ${submission.rejectionReason ?: "unknown_error"}"
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(MukeiSpacing.Large),
        verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Medium),
    ) {
        Text("My Storage", style = MaterialTheme.typography.headlineSmall)
        Text(
            "Folders and files below come from the encrypted Universal Storage tree. File bytes remain in the immutable object store.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        if (loaded == null || currentNode == null) {
            Text("Opening encrypted storage…", style = MaterialTheme.typography.bodyMedium)
            return@Column
        }

        StorageBreadcrumbs(
            snapshot = loaded,
            currentNodeId = currentNode.nodeId,
            onOpen = ::openDirectory,
        )

        banner?.let { message ->
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.medium,
                color = MaterialTheme.colorScheme.surfaceVariant,
            ) {
                Row(
                    modifier = Modifier.padding(MukeiSpacing.Medium),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(
                        text = message,
                        modifier = Modifier.weight(1f),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    TextButton(onClick = { banner = null }) { Text("Dismiss") }
                }
            }
        }

        if (writableDirectory) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.Small),
            ) {
                Button(
                    onClick = { documentPicker.launch(arrayOf("*/*")) },
                    modifier = Modifier.weight(1f),
                ) {
                    Text("Import file")
                }
                Button(
                    onClick = { createFolderOpen = true },
                    modifier = Modifier.weight(1f),
                ) {
                    Text("New folder")
                }
            }
        } else {
            Text(
                if (insideTrash) {
                    "Trash is read-only. Restore an item before editing or importing into it."
                } else {
                    "This directory is protected and cannot accept new content."
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        OutlinedTextField(
            value = searchQuery,
            onValueChange = { searchQuery = it.take(256) },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Search this folder") },
            singleLine = true,
        )

        val children = storageChildren(loaded, currentNode.nodeId)
            .filter { node ->
                searchQuery.isBlank() || node.displayName.contains(searchQuery.trim(), ignoreCase = true)
            }
            .sortedWith(
                compareBy<StorageNodeCard> { it.nodeType != "directory" }
                    .thenBy(String.CASE_INSENSITIVE_ORDER) { it.displayName },
            )

        if (children.isEmpty()) {
            Text(
                if (searchQuery.isBlank()) "This folder is empty." else "No matching items.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
            children.forEachIndexed { index, node ->
                StorageNodeRow(
                    node = node,
                    insideTrash = insideTrash || currentNode.systemRole == "trash",
                    onOpen = { if (node.nodeType == "directory") openDirectory(node.nodeId) },
                    onRename = { renameTarget = node },
                    onTrash = {
                        val result = BackendRuntimeHost.trashStorageNode(node.nodeId)
                        banner = if (result.status == "accepted") {
                            "Moving ${node.displayName} to Trash…"
                        } else {
                            "Move to Trash rejected: ${result.rejectionReason ?: "unknown_error"}"
                        }
                    },
                    onRestore = {
                        val result = BackendRuntimeHost.restoreStorageNode(node.nodeId)
                        banner = if (result.status == "accepted") {
                            "Restoring ${node.displayName}…"
                        } else {
                            "Restore rejected: ${result.rejectionReason ?: "unknown_error"}"
                        }
                    },
                )
                if (index != children.lastIndex) HorizontalDivider()
            }
        }

        Spacer(Modifier.height(MukeiSpacing.Large))
        Text(
            "M1 policy · Imports are currently limited by the native admission pipeline to supported UTF-8 text/source files up to 32 MiB. Delete is intentionally reversible: items move to Trash.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }

    if (createFolderOpen) {
        var folderName by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { createFolderOpen = false },
            title = { Text("New folder") },
            text = {
                OutlinedTextField(
                    value = folderName,
                    onValueChange = { folderName = it.take(MaxStorageNameCharacters) },
                    label = { Text("Folder name") },
                    singleLine = true,
                )
            },
            confirmButton = {
                TextButton(
                    enabled = folderName.trim().isNotEmpty() && currentNodeId != null,
                    onClick = {
                        val parent = currentNodeId ?: return@TextButton
                        val result = BackendRuntimeHost.createStorageDirectory(parent, folderName.trim())
                        if (result.status == "accepted") {
                            createFolderOpen = false
                            banner = "Creating ${folderName.trim()}…"
                        } else {
                            banner = "Folder creation rejected: ${result.rejectionReason ?: "unknown_error"}"
                        }
                    },
                ) { Text("Create") }
            },
            dismissButton = {
                TextButton(onClick = { createFolderOpen = false }) { Text("Cancel") }
            },
        )
    }

    renameTarget?.let { target ->
        var replacementName by remember(target.nodeId) { mutableStateOf(target.displayName) }
        AlertDialog(
            onDismissRequest = { renameTarget = null },
            title = { Text("Rename") },
            text = {
                OutlinedTextField(
                    value = replacementName,
                    onValueChange = { replacementName = it.take(MaxStorageNameCharacters) },
                    label = { Text("Name") },
                    singleLine = true,
                )
            },
            confirmButton = {
                TextButton(
                    enabled = replacementName.trim().isNotEmpty(),
                    onClick = {
                        val result = BackendRuntimeHost.renameStorageNode(
                            target.nodeId,
                            replacementName.trim(),
                        )
                        if (result.status == "accepted") {
                            renameTarget = null
                            banner = "Renaming ${target.displayName}…"
                        } else {
                            banner = "Rename rejected: ${result.rejectionReason ?: "unknown_error"}"
                        }
                    },
                ) { Text("Save") }
            },
            dismissButton = {
                TextButton(onClick = { renameTarget = null }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun StorageBreadcrumbs(
    snapshot: StorageWorkspaceSnapshot,
    currentNodeId: String,
    onOpen: (String) -> Unit,
) {
    val breadcrumbs = storageBreadcrumbs(snapshot, currentNodeId)
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall),
    ) {
        breadcrumbs.forEachIndexed { index, node ->
            TextButton(onClick = { onOpen(node.nodeId) }) {
                Text(if (node.nodeId == snapshot.rootNodeId) "My Storage" else node.displayName)
            }
            if (index != breadcrumbs.lastIndex) {
                Text("/", modifier = Modifier.padding(top = MukeiSpacing.Medium))
            }
        }
    }
}

@Composable
private fun StorageNodeRow(
    node: StorageNodeCard,
    insideTrash: Boolean,
    onOpen: () -> Unit,
    onRename: () -> Unit,
    onTrash: () -> Unit,
    onRestore: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = MukeiSpacing.Medium),
        verticalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall),
    ) {
        Text(
            text = when {
                node.systemRole == "trash" -> "Trash"
                node.nodeType == "directory" -> "Folder · ${node.displayName}"
                else -> node.displayName
            },
            style = MaterialTheme.typography.titleMedium,
        )
        Text(
            text = storageMetadata(node),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall)) {
            if (node.nodeType == "directory") {
                TextButton(onClick = onOpen) { Text("Open") }
            }
            when {
                node.state == "trashed" -> {
                    TextButton(onClick = onRestore) { Text("Restore") }
                }
                node.systemRole == null && !insideTrash && node.state == "active" -> {
                    TextButton(onClick = onRename) { Text("Rename") }
                    TextButton(onClick = onTrash) { Text("Move to Trash") }
                }
            }
        }
    }
}

private fun loadStorageWorkspaceSnapshot(): StorageWorkspaceSnapshot? {
    val raw = BackendRuntimeHost.requestRuntimeSnapshot("storage") ?: return null
    return runCatching {
        val payload = JSONObject(raw).optJSONObject("payload") ?: return@runCatching null
        val scopeId = payload.optString("scope_id")
        val rootNodeId = payload.optString("root_node_id")
        if (scopeId.isBlank() || rootNodeId.isBlank()) return@runCatching null
        val nodesJson = payload.optJSONArray("nodes") ?: JSONArray()
        val nodes = buildList {
            for (index in 0 until nodesJson.length()) {
                val node = nodesJson.optJSONObject(index) ?: continue
                val nodeId = node.optString("node_id")
                if (nodeId.isBlank()) continue
                add(
                    StorageNodeCard(
                        nodeId = nodeId,
                        parentNodeId = nullableJsonString(node, "parent_node_id"),
                        nodeType = node.optString("node_type"),
                        displayName = node.optString("display_name", "Untitled"),
                        state = node.optString("state"),
                        systemRole = nullableJsonString(node, "system_role"),
                        sizeBytes = if (node.isNull("size_bytes")) null else node.optLong("size_bytes"),
                        mimeType = nullableJsonString(node, "mime_type"),
                        updatedAt = node.optString("updated_at"),
                    ),
                )
            }
        }
        StorageWorkspaceSnapshot(scopeId = scopeId, rootNodeId = rootNodeId, nodes = nodes)
    }.getOrNull()
}

private fun storageChildren(
    snapshot: StorageWorkspaceSnapshot,
    parentNodeId: String,
): List<StorageNodeCard> {
    val inTrash = isInsideTrash(snapshot, parentNodeId)
    return snapshot.nodes.filter { node ->
        node.parentNodeId == parentNodeId &&
            node.state != "deleted" &&
            (inTrash || node.state == "active" || node.systemRole == "trash")
    }
}

private fun storageBreadcrumbs(
    snapshot: StorageWorkspaceSnapshot,
    currentNodeId: String,
): List<StorageNodeCard> {
    val byId = snapshot.nodes.associateBy(StorageNodeCard::nodeId)
    val path = mutableListOf<StorageNodeCard>()
    val visited = mutableSetOf<String>()
    var cursor: String? = currentNodeId
    while (cursor != null && visited.add(cursor)) {
        val node = byId[cursor] ?: break
        path += node
        if (node.nodeId == snapshot.rootNodeId) break
        cursor = node.parentNodeId
    }
    if (path.none { it.nodeId == snapshot.rootNodeId }) {
        byId[snapshot.rootNodeId]?.let { path += it }
    }
    return path.asReversed()
}

private fun isInsideTrash(snapshot: StorageWorkspaceSnapshot, nodeId: String): Boolean {
    val byId = snapshot.nodes.associateBy(StorageNodeCard::nodeId)
    val visited = mutableSetOf<String>()
    var cursor: String? = nodeId
    while (cursor != null && visited.add(cursor)) {
        val node = byId[cursor] ?: return false
        if (node.systemRole == "trash") return true
        cursor = node.parentNodeId
    }
    return false
}

private fun storageMetadata(node: StorageNodeCard): String = when {
    node.systemRole == "trash" -> "Protected system folder"
    node.nodeType == "directory" && node.state == "trashed" -> "Folder · In Trash"
    node.nodeType == "directory" -> "Folder"
    else -> listOfNotNull(
        node.mimeType,
        node.sizeBytes?.let(::formatStorageSize),
        if (node.state == "trashed") "In Trash" else null,
    ).joinToString(" · ").ifBlank { "Encrypted file" }
}

private fun formatStorageSize(bytes: Long): String = when {
    bytes < 1024L -> "$bytes B"
    bytes < 1024L * 1024L -> "${bytes / 1024L} KiB"
    else -> "${bytes / (1024L * 1024L)} MiB"
}

private fun nullableJsonString(json: JSONObject, key: String): String? {
    if (!json.has(key) || json.isNull(key)) return null
    return json.optString(key).takeIf { it.isNotBlank() && it != "null" }
}

private fun resolveDocumentDisplayName(context: Context, uri: Uri): String {
    val fromProvider = runCatching {
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { cursor ->
                val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (index >= 0 && cursor.moveToFirst()) cursor.getString(index) else null
            }
    }.getOrNull()
    return fromProvider?.trim()?.takeIf { it.isNotEmpty() }
        ?: uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() }
        ?: "imported-file.txt"
}

private fun storageFailureMessage(code: String): String = when (code) {
    "storage_directory_create_failed" -> "Folder could not be created. Check the name and destination."
    "storage_node_rename_failed" -> "Rename failed. Another item may already use that name."
    "storage_node_trash_failed" -> "Item could not be moved to Trash."
    "storage_node_restore_failed" -> "Restore failed. The original folder may be unavailable or contain a name conflict."
    "storage_import_failed" -> "Import failed native validation or encrypted storage commit."
    else -> "Storage operation failed: $code"
}
