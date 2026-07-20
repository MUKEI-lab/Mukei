package ai.mukei.android

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import ai.mukei.android.designsystem.MukeiSpacing
import ai.mukei.android.designsystem.MukeiStroke
import org.json.JSONObject

internal data class ConversationStorageAttachmentCard(
    val nodeId: String,
    val displayName: String,
    val mimeType: String?,
    val sizeBytes: Long,
    val nodeState: String,
)

internal data class ConversationStoragePickerNode(
    val nodeId: String,
    val parentNodeId: String?,
    val nodeType: String,
    val displayName: String,
    val state: String,
    val systemRole: String?,
    val sizeBytes: Long?,
    val mimeType: String?,
)

private data class ConversationStoragePickerSnapshot(
    val rootNodeId: String,
    val nodes: List<ConversationStoragePickerNode>,
)

internal fun loadConversationStorageAttachments(
    conversationId: String,
): List<ConversationStorageAttachmentCard> {
    val raw = BackendRuntimeHost.requestRuntimeSnapshot("conversations") ?: return emptyList()
    return runCatching {
        val payload = JSONObject(raw).optJSONObject("payload") ?: return@runCatching emptyList()
        val attachments = payload.optJSONArray("attachments") ?: return@runCatching emptyList()
        buildList {
            for (index in 0 until attachments.length()) {
                val item = attachments.optJSONObject(index) ?: continue
                if (item.optString("conversation_id") != conversationId) continue
                val nodeId = item.optString("node_id")
                val displayName = item.optString("display_name")
                if (nodeId.isBlank() || displayName.isBlank()) continue
                add(
                    ConversationStorageAttachmentCard(
                        nodeId = nodeId,
                        displayName = displayName,
                        mimeType = item.optString("mime_type").takeIf { it.isNotBlank() },
                        sizeBytes = item.optLong("size_bytes", 0L).coerceAtLeast(0L),
                        nodeState = item.optString("node_state", "unavailable"),
                    ),
                )
            }
        }
    }.getOrDefault(emptyList())
}

@Composable
internal fun ConversationStoragePickerDialog(
    onDismiss: () -> Unit,
    onSelect: (ConversationStoragePickerNode) -> Unit,
) {
    val snapshot = remember { loadConversationStoragePickerSnapshot() }
    var currentNodeId by remember(snapshot?.rootNodeId) {
        mutableStateOf(snapshot?.rootNodeId)
    }
    var searchQuery by remember { mutableStateOf("") }

    val currentNode = snapshot?.nodes?.firstOrNull { it.nodeId == currentNodeId }
    val children = snapshot
        ?.nodes
        .orEmpty()
        .asSequence()
        .filter { node ->
            node.parentNodeId == currentNodeId &&
                node.state == "active" &&
                node.systemRole != "trash"
        }
        .filter { node ->
            searchQuery.isBlank() || node.displayName.contains(searchQuery.trim(), ignoreCase = true)
        }
        .sortedWith(
            compareBy<ConversationStoragePickerNode> { it.nodeType != "directory" }
                .thenBy(String.CASE_INSENSITIVE_ORDER) { it.displayName },
        )
        .toList()

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Attach from Storage") },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small),
            ) {
                if (snapshot == null || currentNode == null) {
                    Text(
                        "Encrypted Storage is unavailable right now.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    return@Column
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(
                        currentNode.displayName,
                        style = MaterialTheme.typography.titleMedium,
                    )
                    if (currentNode.nodeId != snapshot.rootNodeId) {
                        TextButton(
                            onClick = {
                                currentNode.parentNodeId?.let { parent ->
                                    currentNodeId = parent
                                    searchQuery = ""
                                }
                            },
                        ) { Text("Back") }
                    }
                }

                OutlinedTextField(
                    value = searchQuery,
                    onValueChange = { searchQuery = it.take(256) },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("Search this folder") },
                    singleLine = true,
                )

                if (children.isEmpty()) {
                    Text(
                        if (searchQuery.isBlank()) {
                            "No attachable files in this folder."
                        } else {
                            "No matching items."
                        },
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    children.forEach { node ->
                        Surface(
                            modifier = Modifier.fillMaxWidth(),
                            shape = MaterialTheme.shapes.medium,
                            color = MaterialTheme.colorScheme.surface,
                            border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
                        ) {
                            Column(modifier = Modifier.padding(MukeiSpacing.Medium)) {
                                Text(node.displayName, style = MaterialTheme.typography.titleSmall)
                                Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                                if (node.nodeType == "directory") {
                                    Text(
                                        "Folder",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                    TextButton(
                                        onClick = {
                                            currentNodeId = node.nodeId
                                            searchQuery = ""
                                        },
                                    ) { Text("Open") }
                                } else {
                                    Text(
                                        buildString {
                                            append(node.mimeType ?: "File")
                                            node.sizeBytes?.let { size ->
                                                append(" · ")
                                                append(formatConversationAttachmentSize(size))
                                            }
                                        },
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                    Spacer(Modifier.height(MukeiSpacing.Small))
                                    Button(
                                        onClick = { onSelect(node) },
                                        modifier = Modifier.fillMaxWidth(),
                                    ) { Text("Attach") }
                                }
                            }
                        }
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )
}

internal fun formatConversationAttachmentSize(bytes: Long): String = when {
    bytes >= 1024L * 1024L -> "%.1f MiB".format(bytes.toDouble() / (1024.0 * 1024.0))
    bytes >= 1024L -> "%.1f KiB".format(bytes.toDouble() / 1024.0)
    else -> "$bytes B"
}

private fun loadConversationStoragePickerSnapshot(): ConversationStoragePickerSnapshot? {
    val raw = BackendRuntimeHost.requestRuntimeSnapshot("storage") ?: return null
    return runCatching {
        val payload = JSONObject(raw).optJSONObject("payload") ?: return@runCatching null
        val rootNodeId = payload.optString("root_node_id")
        val nodesJson = payload.optJSONArray("nodes") ?: return@runCatching null
        if (rootNodeId.isBlank()) return@runCatching null
        val nodes = buildList {
            for (index in 0 until nodesJson.length()) {
                val item = nodesJson.optJSONObject(index) ?: continue
                val nodeId = item.optString("node_id")
                if (nodeId.isBlank()) continue
                add(
                    ConversationStoragePickerNode(
                        nodeId = nodeId,
                        parentNodeId = item.optString("parent_node_id").takeIf { it.isNotBlank() },
                        nodeType = item.optString("node_type"),
                        displayName = item.optString("display_name", "Untitled"),
                        state = item.optString("state"),
                        systemRole = item.optString("system_role").takeIf { it.isNotBlank() },
                        sizeBytes = if (item.has("size_bytes") && !item.isNull("size_bytes")) {
                            item.optLong("size_bytes").coerceAtLeast(0L)
                        } else {
                            null
                        },
                        mimeType = item.optString("mime_type").takeIf { it.isNotBlank() },
                    ),
                )
            }
        }
        ConversationStoragePickerSnapshot(rootNodeId = rootNodeId, nodes = nodes)
    }.getOrNull()
}
