package ai.mukei.android

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import ai.mukei.android.designsystem.MukeiSpacing
import ai.mukei.android.designsystem.MukeiStroke
import org.json.JSONArray
import org.json.JSONObject

private const val MaxProjectInstructionsLength = 4_096
private const val MaxProjectMemoryLength = 1_024
private const val MaxProjectMemoryEntries = 16

private data class ProjectMemoryCard(
    val memoryId: String,
    val content: String,
    val createdAt: String,
    val updatedAt: String,
)

private data class ProjectContextSnapshot(
    val instructions: String,
    val memory: List<ProjectMemoryCard>,
)

@Composable
internal fun ProjectContextSurface(
    projectId: String,
    readOnly: Boolean,
) {
    var instructions by remember(projectId) { mutableStateOf("") }
    val memories = remember(projectId) { mutableStateListOf<ProjectMemoryCard>() }
    var banner by remember(projectId) { mutableStateOf<String?>(null) }
    var showInstructionsDialog by remember(projectId) { mutableStateOf(false) }
    var editingMemoryId by remember(projectId) { mutableStateOf<String?>(null) }
    var showMemoryDialog by remember(projectId) { mutableStateOf(false) }

    fun refresh() {
        val context = parseProjectContext(
            raw = BackendRuntimeHost.requestRuntimeSnapshot("projects"),
            projectId = projectId,
        )
        instructions = context?.instructions.orEmpty()
        memories.clear()
        memories.addAll(context?.memory.orEmpty())
    }

    LaunchedEffect(projectId) { refresh() }

    DisposableEffect(projectId) {
        val registration = BackendRuntimeHost.addEventListener { batch ->
            if (batch.events.any { raw ->
                    runCatching {
                        val event = JSONObject(raw)
                        val eventType = event.optString("event_type")
                        val eventProjectId = event.optJSONObject("payload")?.optString("project_id")
                        eventType.startsWith("project.") &&
                            (eventProjectId.isNullOrBlank() || eventProjectId == projectId)
                    }.getOrDefault(false)
                }
            ) {
                refresh()
            }
        }
        onDispose { registration.close() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Medium)) {
        Surface(
            modifier = Modifier.fillMaxWidth(),
            shape = MaterialTheme.shapes.large,
            color = MaterialTheme.colorScheme.surface,
            border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
        ) {
            Column(modifier = Modifier.padding(MukeiSpacing.Large)) {
                Text("Project instructions", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                Text(
                    text = if (instructions.isBlank()) {
                        "No instructions yet. Add persistent guidance for work done inside this project."
                    } else {
                        instructions
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (!readOnly) {
                    Spacer(Modifier.height(MukeiSpacing.Medium))
                    Button(
                        onClick = { showInstructionsDialog = true },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text(if (instructions.isBlank()) "Add instructions" else "Edit instructions")
                    }
                }
            }
        }

        Surface(
            modifier = Modifier.fillMaxWidth(),
            shape = MaterialTheme.shapes.large,
            color = MaterialTheme.colorScheme.surface,
            border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
        ) {
            Column(modifier = Modifier.padding(MukeiSpacing.Large)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("Project memory", style = MaterialTheme.typography.titleMedium)
                    Text(
                        "${memories.size}/$MaxProjectMemoryEntries",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                Text(
                    text = "Memory here belongs only to this project and is stored in its encrypted native projection.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )

                if (!readOnly && memories.size < MaxProjectMemoryEntries) {
                    Spacer(Modifier.height(MukeiSpacing.Medium))
                    Button(
                        onClick = {
                            editingMemoryId = null
                            showMemoryDialog = true
                        },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text("Add memory")
                    }
                }

                if (memories.isEmpty()) {
                    Spacer(Modifier.height(MukeiSpacing.Medium))
                    Text(
                        "No project memory yet.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    Spacer(Modifier.height(MukeiSpacing.Medium))
                    Column(verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small)) {
                        memories.forEach { memory ->
                            Surface(
                                modifier = Modifier.fillMaxWidth(),
                                shape = MaterialTheme.shapes.medium,
                                color = MaterialTheme.colorScheme.surfaceVariant,
                            ) {
                                Column(modifier = Modifier.padding(MukeiSpacing.Medium)) {
                                    Text(memory.content, style = MaterialTheme.typography.bodyMedium)
                                    if (!readOnly) {
                                        Spacer(Modifier.height(MukeiSpacing.Small))
                                        Row(
                                            modifier = Modifier.fillMaxWidth(),
                                            horizontalArrangement = Arrangement.End,
                                        ) {
                                            TextButton(
                                                onClick = {
                                                    editingMemoryId = memory.memoryId
                                                    showMemoryDialog = true
                                                },
                                            ) {
                                                Text("Edit")
                                            }
                                            TextButton(
                                                onClick = {
                                                    val result = BackendRuntimeHost.deleteProjectMemory(
                                                        projectId = projectId,
                                                        memoryId = memory.memoryId,
                                                    )
                                                    banner = if (result.status == "accepted") {
                                                        "Project memory removed."
                                                    } else {
                                                        projectContextFailure(
                                                            result.rejectionReason ?: "project_memory_delete_rejected",
                                                        )
                                                    }
                                                    refresh()
                                                },
                                            ) {
                                                Text("Remove")
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        banner?.let { message ->
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.medium,
                color = MaterialTheme.colorScheme.surfaceVariant,
            ) {
                Text(
                    text = message,
                    modifier = Modifier.padding(MukeiSpacing.Medium),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }

    if (showInstructionsDialog) {
        ProjectTextDialog(
            title = "Project instructions",
            label = "Instructions",
            initialValue = instructions,
            maxLength = MaxProjectInstructionsLength,
            allowEmpty = true,
            confirmLabel = "Save",
            onDismiss = { showInstructionsDialog = false },
            onConfirm = { value ->
                val result = BackendRuntimeHost.updateProjectInstructions(projectId, value)
                if (result.status == "accepted") {
                    showInstructionsDialog = false
                    banner = if (value.isBlank()) "Project instructions cleared." else "Project instructions saved."
                    refresh()
                } else {
                    banner = projectContextFailure(
                        result.rejectionReason ?: "project_instructions_update_rejected",
                    )
                }
            },
        )
    }

    if (showMemoryDialog) {
        val editingMemory = editingMemoryId?.let { id -> memories.firstOrNull { it.memoryId == id } }
        ProjectTextDialog(
            title = if (editingMemory == null) "Add project memory" else "Edit project memory",
            label = "Memory",
            initialValue = editingMemory?.content.orEmpty(),
            maxLength = MaxProjectMemoryLength,
            allowEmpty = false,
            confirmLabel = if (editingMemory == null) "Add" else "Save",
            onDismiss = {
                showMemoryDialog = false
                editingMemoryId = null
            },
            onConfirm = { value ->
                val result = if (editingMemory == null) {
                    BackendRuntimeHost.addProjectMemory(projectId, value)
                } else {
                    BackendRuntimeHost.updateProjectMemory(projectId, editingMemory.memoryId, value)
                }
                if (result.status == "accepted") {
                    showMemoryDialog = false
                    editingMemoryId = null
                    banner = if (editingMemory == null) "Project memory added." else "Project memory updated."
                    refresh()
                } else {
                    banner = projectContextFailure(
                        result.rejectionReason ?: "project_memory_update_rejected",
                    )
                }
            },
        )
    }
}

@Composable
private fun ProjectTextDialog(
    title: String,
    label: String,
    initialValue: String,
    maxLength: Int,
    allowEmpty: Boolean,
    confirmLabel: String,
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    var value by remember(title, initialValue) { mutableStateOf(initialValue) }
    val normalized = value.trim()
    val valid = value.length <= maxLength && (allowEmpty || normalized.isNotEmpty())

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column {
                OutlinedTextField(
                    value = value,
                    onValueChange = { value = it.take(maxLength) },
                    label = { Text(label) },
                    minLines = 5,
                    maxLines = 12,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                Text(
                    "${value.length}/$maxLength",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { onConfirm(normalized) }, enabled = valid) {
                Text(confirmLabel)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )
}

private fun parseProjectContext(raw: String?, projectId: String): ProjectContextSnapshot? {
    if (raw.isNullOrBlank()) return null
    return runCatching {
        val envelope = JSONObject(raw)
        val projects = envelope.optJSONObject("payload")?.optJSONArray("projects") ?: JSONArray()
        for (index in 0 until projects.length()) {
            val project = projects.optJSONObject(index) ?: continue
            if (project.optString("project_id") != projectId) continue
            val memoryValues = project.optJSONArray("memory") ?: JSONArray()
            val memory = buildList {
                for (memoryIndex in 0 until memoryValues.length()) {
                    val value = memoryValues.optJSONObject(memoryIndex) ?: continue
                    val memoryId = value.optString("memory_id").takeIf(String::isNotBlank) ?: continue
                    val content = value.optString("content").takeIf(String::isNotBlank) ?: continue
                    add(
                        ProjectMemoryCard(
                            memoryId = memoryId,
                            content = content,
                            createdAt = value.optString("created_at"),
                            updatedAt = value.optString("updated_at"),
                        ),
                    )
                }
            }
            return@runCatching ProjectContextSnapshot(
                instructions = project.optString("instructions"),
                memory = memory,
            )
        }
        null
    }.getOrNull()
}

private fun projectContextFailure(code: String): String = when (code) {
    "invalid_payload" -> "Project instructions or memory did not pass validation."
    "stale_scope" -> "That project or memory entry no longer exists."
    "policy_denied" -> "Archived projects are read-only, or this project reached its memory limit."
    "backend_unavailable" -> "The secure runtime is not available."
    else -> "Project context operation failed safely ($code)."
}
