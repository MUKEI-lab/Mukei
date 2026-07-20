package ai.mukei.android

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import ai.mukei.android.designsystem.MukeiSpacing
import java.util.UUID

@Composable
internal fun ConversationWorkspaceSurface(
    readiness: AppReadiness,
    resetGeneration: Int,
    initialPrompt: String?,
    onInitialPromptConsumed: () -> Unit,
) {
    var conversationId by remember { mutableStateOf<String?>(null) }
    var branchId by remember { mutableStateOf<String?>(null) }
    var initialOperationId by remember { mutableStateOf<String?>(null) }
    var draft by remember { mutableStateOf("") }
    var banner by remember { mutableStateOf<String?>(null) }
    var selectedProjectId by remember { mutableStateOf<String?>(null) }

    fun resetToNewConversation() {
        conversationId = null
        branchId = null
        initialOperationId = null
        draft = ""
        banner = null
        selectedProjectId = null
    }

    fun startConversation(text: String, projectId: String?) {
        val newConversation = UUID.randomUUID().toString()
        val newBranch = UUID.randomUUID().toString()
        val result = BackendRuntimeHost.sendChatMessage(
            conversationId = newConversation,
            branchId = newBranch,
            text = text,
            projectId = projectId,
        )
        if (result.status == "accepted") {
            conversationId = newConversation
            branchId = newBranch
            initialOperationId = result.operationId
            draft = ""
            banner = null
            selectedProjectId = null
        } else {
            banner = when (result.rejectionReason) {
                "backend_unavailable" -> "A ready model is required before sending."
                "policy_denied" -> "That project cannot be attached to this conversation."
                "stale_scope" -> "That project or conversation scope is no longer available."
                else -> "Message could not be sent: ${result.rejectionReason ?: "rejected"}"
            }
        }
    }

    LaunchedEffect(resetGeneration) {
        if (resetGeneration > 0) resetToNewConversation()
    }
    LaunchedEffect(initialPrompt) {
        val prompt = initialPrompt?.trim().orEmpty()
        if (prompt.isNotEmpty()) {
            onInitialPromptConsumed()
            startConversation(prompt, null)
        }
    }

    val activeConversation = conversationId
    val activeBranch = branchId
    Column(modifier = Modifier.fillMaxSize()) {
        if (activeConversation != null && activeBranch != null) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = MukeiSpacing.Medium),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                TextButton(onClick = { resetToNewConversation() }) { Text("All chats") }
                TextButton(onClick = { resetToNewConversation() }) { Text("New conversation") }
            }
            Box(modifier = Modifier.weight(1f)) {
                ChatConversationSurface(
                    conversationId = activeConversation,
                    branchId = activeBranch,
                    readiness = readiness,
                    initialOperationId = initialOperationId,
                    onBranchChange = { selectedBranch ->
                        val result = BackendRuntimeHost.selectConversationBranch(
                            activeConversation,
                            selectedBranch,
                        )
                        if (result.status == "accepted") {
                            branchId = selectedBranch
                            initialOperationId = null
                        } else {
                            banner = "Branch could not be opened: ${result.rejectionReason ?: "rejected"}"
                        }
                    },
                )
            }
        } else {
            Box(modifier = Modifier.weight(1f)) {
                ChatsSurface { selectedConversation, selectedBranch ->
                    conversationId = selectedConversation
                    branchId = selectedBranch
                    initialOperationId = null
                    selectedProjectId = null
                    banner = null
                }
            }
            banner?.let { message ->
                Text(
                    text = message,
                    modifier = Modifier.padding(horizontal = MukeiSpacing.Large),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(MukeiSpacing.Large),
                verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small),
            ) {
                val projectOptions = loadActiveChatProjects()
                if (projectOptions.isNotEmpty()) {
                    Text("Project context", style = MaterialTheme.typography.labelLarge)
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .horizontalScroll(rememberScrollState()),
                        horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall),
                    ) {
                        TextButton(onClick = { selectedProjectId = null }) {
                            Text(if (selectedProjectId == null) "None · Selected" else "None")
                        }
                        projectOptions.forEach { project ->
                            TextButton(onClick = { selectedProjectId = project.projectId }) {
                                Text(
                                    if (selectedProjectId == project.projectId) {
                                        "${project.name} · Selected"
                                    } else {
                                        project.name
                                    },
                                )
                            }
                        }
                    }
                    Text(
                        "Project binding is fixed when the conversation is first created.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                OutlinedTextField(
                    value = draft,
                    onValueChange = { draft = it.take(64 * 1024) },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("Message Mukei") },
                    minLines = 2,
                    maxLines = 6,
                )
                Button(
                    onClick = { startConversation(draft.trim(), selectedProjectId) },
                    enabled = readiness.inference.status == ReadinessStatus.READY &&
                        draft.trim().isNotEmpty(),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Start conversation")
                }
            }
        }
    }
}
