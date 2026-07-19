package ai.mukei.android

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
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
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import ai.mukei.android.designsystem.MukeiLayout
import ai.mukei.android.designsystem.MukeiSpacing
import ai.mukei.android.designsystem.MukeiStroke
import org.json.JSONArray
import org.json.JSONObject

private enum class ProjectState {
    ACTIVE,
    ARCHIVED,
}

private data class ProjectCard(
    val projectId: String,
    val name: String,
    val description: String,
    val state: ProjectState,
    val createdAt: String,
    val updatedAt: String,
)

@Composable
internal fun ProjectsSurface() {
    val projects = remember { mutableStateListOf<ProjectCard>() }
    var selectedProjectId by remember { mutableStateOf<String?>(null) }
    var showCreateDialog by remember { mutableStateOf(false) }
    var showEditDialog by remember { mutableStateOf(false) }
    var banner by remember { mutableStateOf<String?>(null) }

    fun refresh() {
        val snapshot = BackendRuntimeHost.requestRuntimeSnapshot("projects")
        val parsed = parseProjectsSnapshot(snapshot)
        projects.clear()
        projects.addAll(parsed)
        if (selectedProjectId != null && projects.none { it.projectId == selectedProjectId }) {
            selectedProjectId = null
        }
    }

    LaunchedEffect(Unit) { refresh() }

    DisposableEffect(Unit) {
        val registration = BackendRuntimeHost.addEventListener { batch ->
            if (batch.events.any { raw ->
                    runCatching { JSONObject(raw).optString("event_type") }
                        .getOrNull()
                        ?.startsWith("project.") == true
                }
            ) {
                refresh()
            }
        }
        onDispose { registration.close() }
    }

    val selectedProject = selectedProjectId?.let { id -> projects.firstOrNull { it.projectId == id } }
    if (selectedProject != null) {
        ProjectDetailSurface(
            project = selectedProject,
            banner = banner,
            onBack = {
                selectedProjectId = null
                banner = null
            },
            onEdit = { showEditDialog = true },
            onArchive = {
                val result = BackendRuntimeHost.archiveProject(selectedProject.projectId)
                banner = if (result.status == "accepted") {
                    "Project archived."
                } else {
                    friendlyProjectFailure(result.rejectionReason ?: "project_archive_rejected")
                }
                refresh()
            },
        )
    } else {
        ProjectListSurface(
            projects = projects,
            banner = banner,
            onCreate = { showCreateDialog = true },
            onOpen = {
                selectedProjectId = it.projectId
                banner = null
            },
        )
    }

    if (showCreateDialog) {
        ProjectEditorDialog(
            title = "New project",
            initialName = "",
            initialDescription = "",
            confirmLabel = "Create",
            onDismiss = { showCreateDialog = false },
            onConfirm = { name, description ->
                val result = BackendRuntimeHost.createProject(name, description)
                if (result.status == "accepted") {
                    showCreateDialog = false
                    banner = "Project created."
                    refresh()
                } else {
                    banner = friendlyProjectFailure(result.rejectionReason ?: "project_create_rejected")
                }
            },
        )
    }

    if (showEditDialog && selectedProject != null) {
        ProjectEditorDialog(
            title = "Edit project",
            initialName = selectedProject.name,
            initialDescription = selectedProject.description,
            confirmLabel = "Save",
            onDismiss = { showEditDialog = false },
            onConfirm = { name, description ->
                val result = BackendRuntimeHost.updateProject(
                    projectId = selectedProject.projectId,
                    name = name,
                    description = description,
                )
                if (result.status == "accepted") {
                    showEditDialog = false
                    banner = "Project updated."
                    refresh()
                } else {
                    banner = friendlyProjectFailure(result.rejectionReason ?: "project_update_rejected")
                }
            },
        )
    }
}

@Composable
private fun ProjectListSurface(
    projects: List<ProjectCard>,
    banner: String?,
    onCreate: () -> Unit,
    onOpen: (ProjectCard) -> Unit,
) {
    val active = projects.filter { it.state == ProjectState.ACTIVE }
    val archived = projects.filter { it.state == ProjectState.ARCHIVED }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(MukeiLayout.LargePhoneTextPadding),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .widthIn(max = MukeiLayout.ReadableContentMaxWidth),
        ) {
            Text("Projects", style = MaterialTheme.typography.headlineMedium)
            Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
            Text(
                text = "Keep long-running work organized in durable, encrypted project records.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(MukeiSpacing.Large))
            Button(onClick = onCreate, modifier = Modifier.fillMaxWidth()) {
                Text("Create project")
            }

            banner?.let { message ->
                Spacer(Modifier.height(MukeiSpacing.Medium))
                ProjectBanner(message)
            }

            Spacer(Modifier.height(MukeiSpacing.Large))
            Text(
                text = if (active.isEmpty()) "No active projects yet" else "Active projects",
                style = MaterialTheme.typography.titleMedium,
            )
            Spacer(Modifier.height(MukeiSpacing.Small))

            if (active.isEmpty()) {
                Surface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = MaterialTheme.shapes.large,
                    color = MaterialTheme.colorScheme.surface,
                    border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
                ) {
                    Text(
                        text = "Create a project to give related chats and files a durable home.",
                        modifier = Modifier.padding(MukeiSpacing.Large),
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small)) {
                    active.forEach { project -> ProjectListCard(project, onOpen) }
                }
            }

            if (archived.isNotEmpty()) {
                Spacer(Modifier.height(MukeiSpacing.Large))
                Text("Archived", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(MukeiSpacing.Small))
                Column(verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small)) {
                    archived.forEach { project -> ProjectListCard(project, onOpen) }
                }
            }
            Spacer(Modifier.height(MukeiSpacing.Major))
        }
    }
}

@Composable
private fun ProjectListCard(project: ProjectCard, onOpen: (ProjectCard) -> Unit) {
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clickable { onOpen(project) },
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
    ) {
        Column(modifier = Modifier.padding(MukeiSpacing.Large)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = project.name,
                    modifier = Modifier.weight(1f),
                    style = MaterialTheme.typography.titleLarge,
                )
                if (project.state == ProjectState.ARCHIVED) {
                    Text(
                        text = "Archived",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (project.description.isNotBlank()) {
                Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                Text(
                    text = project.description,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun ProjectDetailSurface(
    project: ProjectCard,
    banner: String?,
    onBack: () -> Unit,
    onEdit: () -> Unit,
    onArchive: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(MukeiLayout.LargePhoneTextPadding),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .widthIn(max = MukeiLayout.ReadableContentMaxWidth),
        ) {
            TextButton(onClick = onBack) { Text("Back to projects") }
            Spacer(Modifier.height(MukeiSpacing.Small))
            Text(project.name, style = MaterialTheme.typography.headlineMedium)
            if (project.description.isNotBlank()) {
                Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                Text(
                    text = project.description,
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(MukeiSpacing.Large))

            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.large,
                color = MaterialTheme.colorScheme.surface,
                border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
            ) {
                Column(modifier = Modifier.padding(MukeiSpacing.Large)) {
                    Text("Project workspace", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                    Text(
                        text = "This project identity is persisted by the encrypted native runtime. Chat and file membership will attach to this identity instead of creating a second storage system.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            Spacer(Modifier.height(MukeiSpacing.Medium))
    ProjectContextSurface(
        projectId = project.projectId,
        readOnly = project.state == ProjectState.ARCHIVED,
    )

            banner?.let { message ->
                Spacer(Modifier.height(MukeiSpacing.Medium))
                ProjectBanner(message)
            }

            if (project.state == ProjectState.ACTIVE) {
                Spacer(Modifier.height(MukeiSpacing.Large))
                Button(onClick = onEdit, modifier = Modifier.fillMaxWidth()) {
                    Text("Edit project")
                }
                Spacer(Modifier.height(MukeiSpacing.Small))
                TextButton(onClick = onArchive, modifier = Modifier.fillMaxWidth()) {
                    Text("Archive project")
                }
            } else {
                Spacer(Modifier.height(MukeiSpacing.Large))
                Text(
                    text = "Archived projects are read-only in this MVP.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(MukeiSpacing.Major))
        }
    }
}

@Composable
private fun ProjectEditorDialog(
    title: String,
    initialName: String,
    initialDescription: String,
    confirmLabel: String,
    onDismiss: () -> Unit,
    onConfirm: (String, String) -> Unit,
) {
    var name by remember(initialName) { mutableStateOf(initialName) }
    var description by remember(initialDescription) { mutableStateOf(initialDescription) }
    val valid = name.trim().isNotEmpty() && name.trim().length <= 128 && description.length <= 4_096

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small)) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it.take(128) },
                    label = { Text("Name") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = description,
                    onValueChange = { description = it.take(4_096) },
                    label = { Text("Description") },
                    minLines = 3,
                    maxLines = 6,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onConfirm(name.trim(), description.trim()) },
                enabled = valid,
            ) {
                Text(confirmLabel)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )
}

@Composable
private fun ProjectBanner(message: String) {
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

private fun parseProjectsSnapshot(raw: String?): List<ProjectCard> {
    if (raw.isNullOrBlank()) return emptyList()
    return runCatching {
        val envelope = JSONObject(raw)
        val payload = envelope.optJSONObject("payload") ?: return@runCatching emptyList()
        val values = payload.optJSONArray("projects") ?: JSONArray()
        buildList {
            for (index in 0 until values.length()) {
                val value = values.optJSONObject(index) ?: continue
                val projectId = value.optString("project_id").takeIf(String::isNotBlank) ?: continue
                val name = value.optString("name").takeIf(String::isNotBlank) ?: continue
                add(
                    ProjectCard(
                        projectId = projectId,
                        name = name,
                        description = value.optString("description"),
                        state = if (value.optString("status") == "archived") {
                            ProjectState.ARCHIVED
                        } else {
                            ProjectState.ACTIVE
                        },
                        createdAt = value.optString("created_at"),
                        updatedAt = value.optString("updated_at"),
                    ),
                )
            }
        }
    }.getOrDefault(emptyList())
}

private fun friendlyProjectFailure(code: String): String = when (code) {
    "invalid_payload" -> "Project name or description is invalid."
    "stale_scope" -> "That project no longer exists."
    "policy_denied" -> "Archived projects cannot be edited."
    "backend_unavailable" -> "The secure runtime is not available."
    else -> "Project operation failed safely ($code)."
}
