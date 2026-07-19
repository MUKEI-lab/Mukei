package ai.mukei.android

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import ai.mukei.android.designsystem.MukeiLayout
import ai.mukei.android.designsystem.MukeiSpacing
import ai.mukei.android.designsystem.MukeiStroke
import org.json.JSONArray
import org.json.JSONObject

private const val MaxChatMessageLength = 64 * 1024

private data class ChatMessageCard(
    val messageId: String,
    val role: String,
    val content: String,
    val createdAt: String,
)

private data class ChatBranchCard(
    val conversationId: String,
    val branchId: String,
    val messages: List<ChatMessageCard>,
) {
    val lastTimestamp: String
        get() = messages.lastOrNull()?.createdAt.orEmpty()
}

private data class ChatSummary(
    val conversationId: String,
    val branchId: String,
    val title: String,
    val preview: String,
    val branchCount: Int,
    val lastTimestamp: String,
)

@Composable
internal fun ChatsSurface(
    onOpenChat: (conversationId: String, branchId: String) -> Unit,
) {
    var branches by remember { mutableStateOf(loadChatBranches()) }

    fun refresh() {
        branches = loadChatBranches()
    }

    LaunchedEffect(Unit) { refresh() }
    DisposableEffect(Unit) {
        val registration = BackendRuntimeHost.addEventListener { batch ->
            if (batch.events.any { raw ->
                    runCatching {
                        JSONObject(raw).optString("event_type").startsWith("chat.")
                    }.getOrDefault(false)
                }
            ) {
                refresh()
            }
        }
        onDispose { registration.close() }
    }

    val summaries = remember(branches) { summarizeChats(branches) }
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(MukeiLayout.LargePhoneTextPadding),
        verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Medium),
    ) {
        if (summaries.isEmpty()) {
            Text("No chats yet", style = MaterialTheme.typography.headlineSmall)
            Text(
                "Start a message from Home. Conversations and their branches will appear here.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
            summaries.forEach { chat ->
                Surface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = MaterialTheme.shapes.large,
                    color = MaterialTheme.colorScheme.surface,
                    border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
                ) {
                    Column(modifier = Modifier.padding(MukeiSpacing.Large)) {
                        Text(chat.title, style = MaterialTheme.typography.titleMedium)
                        Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                        Text(
                            chat.preview,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(MukeiSpacing.Small))
                        Text(
                            "${chat.branchCount} branch${if (chat.branchCount == 1) "" else "es"}",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(MukeiSpacing.Medium))
                        Button(
                            onClick = { onOpenChat(chat.conversationId, chat.branchId) },
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text("Open chat")
                        }
                    }
                }
            }
        }
    }
}

@Composable
internal fun ChatConversationSurface(
    conversationId: String,
    branchId: String,
    readiness: AppReadiness,
    initialOperationId: String?,
    onBranchChange: (String) -> Unit,
) {
    var branches by remember(conversationId) { mutableStateOf(loadChatBranches(conversationId)) }
    var draft by remember(conversationId) { mutableStateOf("") }
    var activeOperationId by remember(conversationId) { mutableStateOf(initialOperationId) }
    var streamingText by remember(conversationId) { mutableStateOf("") }
    var banner by remember(conversationId) { mutableStateOf<String?>(null) }
    var editing by remember(conversationId) { mutableStateOf<ChatMessageCard?>(null) }
    val clipboard = LocalClipboardManager.current

    fun refresh() {
        branches = loadChatBranches(conversationId)
    }

    LaunchedEffect(conversationId, branchId) { refresh() }
    DisposableEffect(conversationId, activeOperationId) {
        val registration = BackendRuntimeHost.addEventListener { batch ->
            var shouldRefresh = false
            batch.events.forEach { raw ->
                runCatching {
                    val event = JSONObject(raw)
                    val eventType = event.optString("event_type")
                    val operationId = event.optString("operation_id").takeIf(String::isNotBlank)
                    val payload = event.optJSONObject("payload") ?: JSONObject()
                    when {
                        eventType == "chat.branch.forked" &&
                            payload.optString("conversation_id") == conversationId -> {
                            val newBranchId = payload.optString("new_branch_id")
                            if (newBranchId.isNotBlank()) onBranchChange(newBranchId)
                            shouldRefresh = true
                        }
                        eventType == "chat.token.delta" && operationId == activeOperationId -> {
                            streamingText += payload.optString("text")
                        }
                        operationId == activeOperationId &&
                            eventType in setOf(
                                "chat.generation.completed",
                                "operation.completed",
                                "operation.failed",
                                "operation.cancelled",
                            ) -> {
                            if (eventType == "operation.failed") {
                                banner = payload.optString("code", "generation_failed")
                            }
                            if (eventType.startsWith("operation.")) {
                                activeOperationId = null
                                streamingText = ""
                            }
                            shouldRefresh = true
                        }
                        eventType.startsWith("chat.") -> shouldRefresh = true
                    }
                }
            }
            if (shouldRefresh) refresh()
        }
        onDispose { registration.close() }
    }

    val orderedBranches = branches.sortedWith(
        compareBy<ChatBranchCard> { it.lastTimestamp }.thenBy { it.branchId },
    )
    val activeBranch = orderedBranches.firstOrNull { it.branchId == branchId }
        ?: orderedBranches.lastOrNull()
    val activeIndex = orderedBranches.indexOfFirst { it.branchId == activeBranch?.branchId }
    val messages = activeBranch?.messages.orEmpty()
    val lastAssistantId = messages.lastOrNull { it.role == "assistant" }?.messageId
    val canGenerate = readiness.inference.status == ReadinessStatus.READY

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(MukeiLayout.LargePhoneTextPadding),
        verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Medium),
    ) {
        if (orderedBranches.size > 1 && activeIndex >= 0) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                TextButton(
                    enabled = activeIndex > 0 && activeOperationId == null,
                    onClick = { onBranchChange(orderedBranches[activeIndex - 1].branchId) },
                ) { Text("Previous") }
                Text(
                    "Branch ${activeIndex + 1} / ${orderedBranches.size}",
                    style = MaterialTheme.typography.labelLarge,
                )
                TextButton(
                    enabled = activeIndex < orderedBranches.lastIndex && activeOperationId == null,
                    onClick = { onBranchChange(orderedBranches[activeIndex + 1].branchId) },
                ) { Text("Next") }
            }
        }

        messages.forEach { message ->
            MessageCard(
                message = message,
                canRegenerate = canGenerate &&
                    activeOperationId == null &&
                    message.role == "assistant" &&
                    message.messageId == lastAssistantId,
                canEdit = activeOperationId == null && message.role in setOf("user", "assistant"),
                onCopy = { clipboard.setText(AnnotatedString(message.content)) },
                onEdit = { editing = message },
                onRegenerate = {
                    val result = BackendRuntimeHost.regenerateChat(conversationId, branchId)
                    if (result.status == "accepted") {
                        activeOperationId = result.operationId
                        streamingText = ""
                        banner = null
                    } else {
                        banner = chatFailure(result.rejectionReason ?: "regenerate_rejected")
                    }
                },
            )
        }

        if (streamingText.isNotBlank()) {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.large,
                color = MaterialTheme.colorScheme.surface,
                border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
            ) {
                Column(modifier = Modifier.padding(MukeiSpacing.Large)) {
                    Text("Mukei", style = MaterialTheme.typography.labelLarge)
                    Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                    Text(streamingText, style = MaterialTheme.typography.bodyLarge)
                }
            }
        }

        banner?.let { message ->
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.medium,
                color = MaterialTheme.colorScheme.surfaceVariant,
            ) {
                Text(message, modifier = Modifier.padding(MukeiSpacing.Medium))
            }
        }

        OutlinedTextField(
            value = draft,
            onValueChange = { draft = it.take(MaxChatMessageLength) },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Message Mukei") },
            minLines = 2,
            maxLines = 8,
            enabled = activeOperationId == null,
        )
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.End,
        ) {
            if (activeOperationId != null) {
                TextButton(
                    onClick = {
                        val result = BackendRuntimeHost.stopChatGeneration(
                            conversationId = conversationId,
                            branchId = branchId,
                            operationId = activeOperationId!!,
                        )
                        if (result.status != "accepted") {
                            banner = chatFailure(result.rejectionReason ?: "stop_rejected")
                        }
                    },
                ) { Text("Stop") }
            } else {
                Button(
                    enabled = canGenerate && draft.trim().isNotEmpty(),
                    onClick = {
                        val text = draft.trim()
                        val result = BackendRuntimeHost.sendChatMessage(
                            conversationId = conversationId,
                            branchId = branchId,
                            text = text,
                        )
                        if (result.status == "accepted") {
                            activeOperationId = result.operationId
                            streamingText = ""
                            draft = ""
                            banner = null
                            refresh()
                        } else {
                            banner = chatFailure(result.rejectionReason ?: "send_rejected")
                        }
                    },
                ) { Text("Send") }
            }
        }
    }

    editing?.let { message ->
        EditMessageDialog(
            message = message,
            onDismiss = { editing = null },
            onConfirm = { text ->
                val result = BackendRuntimeHost.editChatMessage(
                    conversationId = conversationId,
                    branchId = branchId,
                    messageId = message.messageId,
                    text = text,
                )
                if (result.status == "accepted") {
                    activeOperationId = if (message.role == "user") result.operationId else null
                    streamingText = ""
                    banner = "Edited into a new branch. Original history is unchanged."
                    editing = null
                    refresh()
                } else {
                    banner = chatFailure(result.rejectionReason ?: "edit_rejected")
                }
            },
        )
    }
}

@Composable
private fun MessageCard(
    message: ChatMessageCard,
    canRegenerate: Boolean,
    canEdit: Boolean,
    onCopy: () -> Unit,
    onEdit: () -> Unit,
    onRegenerate: () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.large,
        color = if (message.role == "user") {
            MaterialTheme.colorScheme.surfaceVariant
        } else {
            MaterialTheme.colorScheme.surface
        },
        border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
    ) {
        Column(modifier = Modifier.padding(MukeiSpacing.Large)) {
            Text(
                if (message.role == "user") "You" else "Mukei",
                style = MaterialTheme.typography.labelLarge,
            )
            Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
            Text(message.content, style = MaterialTheme.typography.bodyLarge)
            Spacer(Modifier.height(MukeiSpacing.Small))
            Row(horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall)) {
                TextButton(onClick = onCopy) { Text("Copy") }
                if (canEdit) TextButton(onClick = onEdit) { Text("Edit") }
                if (canRegenerate) TextButton(onClick = onRegenerate) { Text("Regenerate") }
            }
        }
    }
}

@Composable
private fun EditMessageDialog(
    message: ChatMessageCard,
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    var text by remember(message.messageId) { mutableStateOf(message.content) }
    val normalized = text.trim()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Edit message") },
        text = {
            Column {
                Text(
                    "Editing creates a new branch. The original message and replies stay unchanged.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(MukeiSpacing.Small))
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it.take(MaxChatMessageLength) },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 4,
                    maxLines = 12,
                )
            }
        },
        confirmButton = {
            TextButton(
                enabled = normalized.isNotEmpty(),
                onClick = { onConfirm(normalized) },
            ) { Text("Save to new branch") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

private fun loadChatBranches(conversationId: String? = null): List<ChatBranchCard> {
    val raw = BackendRuntimeHost.requestRuntimeSnapshot("conversations") ?: return emptyList()
    return runCatching {
        val payload = JSONObject(raw).optJSONObject("payload") ?: JSONObject()
        val values = payload.optJSONArray("branches") ?: JSONArray()
        buildList {
            for (index in 0 until values.length()) {
                val branch = values.optJSONObject(index) ?: continue
                val currentConversationId = branch.optString("conversation_id")
                if (currentConversationId.isBlank()) continue
                if (conversationId != null && currentConversationId != conversationId) continue
                val branchId = branch.optString("branch_id")
                if (branchId.isBlank()) continue
                val messagesJson = branch.optJSONArray("messages") ?: JSONArray()
                val messages = buildList {
                    for (messageIndex in 0 until messagesJson.length()) {
                        val message = messagesJson.optJSONObject(messageIndex) ?: continue
                        val id = message.optString("id")
                        val role = message.optString("role")
                        val content = message.optString("content")
                        if (id.isBlank() || role.isBlank()) continue
                        add(
                            ChatMessageCard(
                                messageId = id,
                                role = role,
                                content = content,
                                createdAt = message.optString("created_at"),
                            ),
                        )
                    }
                }
                add(ChatBranchCard(currentConversationId, branchId, messages))
            }
        }
    }.getOrDefault(emptyList())
}

private fun summarizeChats(branches: List<ChatBranchCard>): List<ChatSummary> = branches
    .groupBy { it.conversationId }
    .mapNotNull { (conversationId, values) ->
        val latest = values.maxWithOrNull(
            compareBy<ChatBranchCard> { it.lastTimestamp }.thenBy { it.branchId },
        ) ?: return@mapNotNull null
        val firstUser = latest.messages.firstOrNull { it.role == "user" }?.content.orEmpty()
        val last = latest.messages.lastOrNull()?.content.orEmpty()
        ChatSummary(
            conversationId = conversationId,
            branchId = latest.branchId,
            title = firstUser.ifBlank { "Conversation" }.lineSequence().first().take(72),
            preview = last.ifBlank { firstUser }.replace('\n', ' ').take(160),
            branchCount = values.size,
            lastTimestamp = latest.lastTimestamp,
        )
    }
    .sortedByDescending { it.lastTimestamp }

private fun chatFailure(code: String): String = when (code) {
    "backend_unavailable" -> "A ready model is required for this action."
    "stale_scope" -> "This conversation branch is no longer available."
    "policy_denied" -> "This message cannot be edited or regenerated."
    "invalid_payload" -> "The message could not be submitted."
    else -> "Chat action failed: $code"
}
