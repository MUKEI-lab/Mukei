from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one anchor, found {count}: {old[:120]!r}")
    file.write_text(text.replace(old, new, 1))


host = "android/app/src/main/kotlin/ai/mukei/android/BackendRuntimeHost.kt"
chat = "android/app/src/main/kotlin/ai/mukei/android/ChatConversationSurface.kt"

# Protocol 2.5 conversation-level attachment commands.
replace_once(
    host,
    '''    fun selectConversationBranch(
        conversationId: String,
        branchId: String,
    ): ChatCommandSubmission = submitConversationCommand(
        commandType = "conversation.select_branch",
        conversationId = conversationId,
        branchId = branchId,
        payload = JSONObject(),
    )

''',
    '''    fun selectConversationBranch(
        conversationId: String,
        branchId: String,
    ): ChatCommandSubmission = submitConversationCommand(
        commandType = "conversation.select_branch",
        conversationId = conversationId,
        branchId = branchId,
        payload = JSONObject(),
    )

    fun addConversationAttachment(
        conversationId: String,
        nodeId: String,
    ): ChatCommandSubmission = submitConversationCommand(
        commandType = "conversation.attachment.add",
        conversationId = conversationId,
        payload = JSONObject().put("node_id", nodeId),
    )

    fun removeConversationAttachment(
        conversationId: String,
        nodeId: String,
    ): ChatCommandSubmission = submitConversationCommand(
        commandType = "conversation.attachment.remove",
        conversationId = conversationId,
        payload = JSONObject().put("node_id", nodeId),
    )

''',
)
replace_once(
    host,
    '.put("protocol_version", JSONObject().put("major", 2).put("minor", 3))\n                .put("command_id", UUID.randomUUID().toString())\n                .put("request_id", UUID.randomUUID().toString())\n                .put("command_type", commandType)\n                .put("submitted_at", Instant.now().toString())\n                .put("correlation_id", UUID.randomUUID().toString())\n                .put("idempotency_key", "conversation-${UUID.randomUUID()}")\n',
    '.put("protocol_version", JSONObject().put("major", 2).put("minor", 5))\n                .put("command_id", UUID.randomUUID().toString())\n                .put("request_id", UUID.randomUUID().toString())\n                .put("command_type", commandType)\n                .put("submitted_at", Instant.now().toString())\n                .put("correlation_id", UUID.randomUUID().toString())\n                .put("idempotency_key", "conversation-${UUID.randomUUID()}")\n',
)
replace_once(
    host,
    '''            val branches = payload.optJSONArray("conversation_branches")
            val conversations = payload.optJSONArray("conversations")
            envelope.put(
                "payload",
                JSONObject()
                    .put("conversations", conversations)
                    .put("branches", branches),
            )
''',
    '''            val branches = payload.optJSONArray("conversation_branches")
            val conversations = payload.optJSONArray("conversations")
            val attachments = payload.optJSONArray("conversation_attachments")
            envelope.put(
                "payload",
                JSONObject()
                    .put("conversations", conversations)
                    .put("branches", branches)
                    .put("attachments", attachments),
            )
''',
)

# Conversation UI owns authoritative attachment projection and picker state.
replace_once(
    chat,
    '''    var banner by remember(conversationId) { mutableStateOf<String?>(null) }
    var editing by remember(conversationId) { mutableStateOf<ChatMessageCard?>(null) }
    val clipboard = LocalClipboardManager.current

    fun refresh() {
        branches = loadChatBranches(conversationId)
    }
''',
    '''    var banner by remember(conversationId) { mutableStateOf<String?>(null) }
    var editing by remember(conversationId) { mutableStateOf<ChatMessageCard?>(null) }
    var attachments by remember(conversationId) {
        mutableStateOf(loadConversationStorageAttachments(conversationId))
    }
    var storagePickerOpen by remember(conversationId) { mutableStateOf(false) }
    val clipboard = LocalClipboardManager.current

    fun refresh() {
        branches = loadChatBranches(conversationId)
        attachments = loadConversationStorageAttachments(conversationId)
    }
''',
)
replace_once(
    chat,
    '''                    val eventType = event.optString("event_type")
                    val operationId = event.optString("operation_id").takeIf(String::isNotBlank)
                    val payload = event.optJSONObject("payload") ?: JSONObject()
                    when {
''',
    '''                    val eventType = event.optString("event_type")
                    val commandType = event.optString("command_type")
                    val operationId = event.optString("operation_id").takeIf(String::isNotBlank)
                    val payload = event.optJSONObject("payload") ?: JSONObject()
                    when {
''',
)
replace_once(
    chat,
    '''                        eventType == "chat.token.delta" && operationId == activeOperationId -> {
                            streamingText += payload.optString("text")
                        }
''',
    '''                        eventType == "chat.token.delta" && operationId == activeOperationId -> {
                            streamingText += payload.optString("text")
                        }
                        eventType == "operation.failed" &&
                            commandType.startsWith("conversation.attachment.") -> {
                            banner = chatFailure(
                                payload.optString("code", "conversation_attachment_failed"),
                            )
                            shouldRefresh = true
                        }
                        eventType.startsWith("conversation.attachment.") ||
                            eventType.startsWith("storage.") -> shouldRefresh = true
''',
)
replace_once(
    chat,
    '''    val lastAssistantId = messages.lastOrNull { it.role == "assistant" }?.messageId
    val isArchived = record?.status == "archived"
    val canGenerate = readiness.inference.status == ReadinessStatus.READY && !isArchived
''',
    '''    val lastAssistantId = messages.lastOrNull { it.role == "assistant" }?.messageId
    val isArchived = record?.status == "archived"
    val attachmentsAvailable = attachments.all { it.nodeState == "active" }
    val canGenerate = readiness.inference.status == ReadinessStatus.READY &&
        !isArchived &&
        attachmentsAvailable
''',
)

# Render attachment references immediately above the composer.
replace_once(
    chat,
    '''        banner?.let { message ->
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.medium,
                color = MaterialTheme.colorScheme.surfaceVariant,
            ) {
                Text(message, modifier = Modifier.padding(MukeiSpacing.Medium))
            }
        }

        OutlinedTextField(
''',
    '''        banner?.let { message ->
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.medium,
                color = MaterialTheme.colorScheme.surfaceVariant,
            ) {
                Text(message, modifier = Modifier.padding(MukeiSpacing.Medium))
            }
        }

        Text("Files from Storage", style = MaterialTheme.typography.titleMedium)
        if (attachments.isEmpty()) {
            Text(
                "No Storage files attached to this conversation.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
            attachments.forEach { attachment ->
                Surface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = MaterialTheme.shapes.medium,
                    color = MaterialTheme.colorScheme.surfaceVariant,
                ) {
                    Column(modifier = Modifier.padding(MukeiSpacing.Medium)) {
                        Text(attachment.displayName, style = MaterialTheme.typography.titleSmall)
                        Text(
                            buildString {
                                append(attachment.mimeType ?: "File")
                                append(" · ")
                                append(formatConversationAttachmentSize(attachment.sizeBytes))
                                if (attachment.nodeState != "active") {
                                    append(" · unavailable")
                                }
                            },
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        if (attachment.nodeState != "active") {
                            Text(
                                "Restore this file in Storage or remove the attachment before sending.",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.error,
                            )
                        }
                        TextButton(
                            enabled = !isArchived && activeOperationId == null,
                            onClick = {
                                val result = BackendRuntimeHost.removeConversationAttachment(
                                    conversationId = conversationId,
                                    nodeId = attachment.nodeId,
                                )
                                banner = if (result.status == "accepted") {
                                    "Removing ${attachment.displayName} from this conversation…"
                                } else {
                                    chatFailure(result.rejectionReason ?: "attachment_remove_rejected")
                                }
                            },
                        ) { Text("Remove") }
                    }
                }
            }
        }
        Button(
            enabled = record != null && !isArchived && activeOperationId == null,
            onClick = { storagePickerOpen = true },
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Attach from Storage") }
        if (!attachmentsAvailable) {
            Text(
                "Sending is blocked while an attached Storage file is unavailable.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }

        OutlinedTextField(
''',
)

# Picker submits only a logical node reference; file bytes remain in Universal Storage.
replace_once(
    chat,
    '''    editing?.let { message ->
        EditMessageDialog(
''',
    '''    if (storagePickerOpen) {
        ConversationStoragePickerDialog(
            onDismiss = { storagePickerOpen = false },
            onSelect = { node ->
                val result = BackendRuntimeHost.addConversationAttachment(
                    conversationId = conversationId,
                    nodeId = node.nodeId,
                )
                if (result.status == "accepted") {
                    storagePickerOpen = false
                    banner = "Attaching ${node.displayName} from encrypted Storage…"
                } else {
                    banner = chatFailure(result.rejectionReason ?: "attachment_add_rejected")
                }
            },
        )
    }

    editing?.let { message ->
        EditMessageDialog(
''',
)

print("conversation Storage attachment Android patch applied")
