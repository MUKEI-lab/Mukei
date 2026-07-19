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
import androidx.compose.material3.FloatingActionButton
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import ai.mukei.android.designsystem.MukeiSpacing
import java.util.UUID

@Composable
internal fun MukeiAppShell(backendState: BackendRuntimeHost.State) {
    var chatsOpen by remember { mutableStateOf(false) }
    var conversationId by remember { mutableStateOf<String?>(null) }
    var branchId by remember { mutableStateOf<String?>(null) }
    var initialOperationId by remember { mutableStateOf<String?>(null) }
    var draft by remember { mutableStateOf("") }
    var banner by remember { mutableStateOf<String?>(null) }
    var selectedProjectId by remember { mutableStateOf<String?>(null) }

    Box(modifier = Modifier.fillMaxSize()) {
        MukeiProductShell(backendState)

        if (backendState is BackendRuntimeHost.State.Ready && !chatsOpen) {
            FloatingActionButton(
                onClick = { chatsOpen = true },
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(MukeiSpacing.Large),
            ) {
                Text("Chats")
            }
        }

        if (backendState is BackendRuntimeHost.State.Ready && chatsOpen) {
            Surface(
                modifier = Modifier.fillMaxSize(),
                color = MaterialTheme.colorScheme.background,
            ) {
                Column(modifier = Modifier.fillMaxSize()) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = MukeiSpacing.Small),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        TextButton(
                            onClick = {
                                if (conversationId != null) {
                                    conversationId = null
                                    branchId = null
                                    initialOperationId = null
                                } else {
                                    chatsOpen = false
                                }
                            },
                        ) {
                            Text(if (conversationId != null) "All chats" else "Close")
                        }
                        Text("Chats", style = MaterialTheme.typography.titleLarge)
                        TextButton(
                            onClick = {
                                conversationId = null
                                branchId = null
                                initialOperationId = null
                                selectedProjectId = null
                                banner = null
                            },
                        ) {
                            Text("New")
                        }
                    }

                    val activeConversation = conversationId
                    val activeBranch = branchId
                    if (activeConversation != null && activeBranch != null) {
                        Box(modifier = Modifier.weight(1f)) {
                            ChatConversationSurface(
                                conversationId = activeConversation,
                                branchId = activeBranch,
                                readiness = backendState.readiness,
                                initialOperationId = initialOperationId,
                                onBranchChange = { selectedBranch ->
                                    branchId = selectedBranch
                                    initialOperationId = null
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
                                Text(
                                    "Project context",
                                    style = MaterialTheme.typography.labelLarge,
                                )
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
                                        TextButton(
                                            onClick = { selectedProjectId = project.projectId },
                                        ) {
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
                                    "A project can only be attached when this conversation is first created. The binding cannot be changed later.",
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
                                onClick = {
                                    val text = draft.trim()
                                    val newConversation = UUID.randomUUID().toString()
                                    val newBranch = UUID.randomUUID().toString()
                                    val result = BackendRuntimeHost.sendChatMessage(
                                        conversationId = newConversation,
                                        branchId = newBranch,
                                        text = text,
                                        projectId = selectedProjectId,
                                    )
                                    if (result.status == "accepted") {
                                        draft = ""
                                        banner = null
                                        conversationId = newConversation
                                        branchId = newBranch
                                        initialOperationId = result.operationId
                                        selectedProjectId = null
                                    } else {
                                        banner = when (result.rejectionReason) {
                                            "backend_unavailable" -> "A ready model is required before sending."
                                            "policy_denied" -> "That project cannot be attached to this conversation."
                                            "stale_scope" -> "That project is no longer available."
                                            else -> "Message could not be sent: ${result.rejectionReason ?: "rejected"}"
                                        }
                                    }
                                },
                                enabled = draft.trim().isNotEmpty(),
                                modifier = Modifier.fillMaxWidth(),
                            ) {
                                Text("Send")
                            }
                        }
                    }
                }
            }
        }
    }
}
