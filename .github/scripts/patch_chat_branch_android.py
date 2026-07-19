from pathlib import Path


def edit(path, fn):
    p = Path(path)
    src = p.read_text(encoding="utf-8")
    out = fn(src)
    if out == src:
        raise SystemExit(f"{path}: patch produced no change")
    p.write_text(out, encoding="utf-8")


def once(src, old, new, label):
    count = src.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 anchor, got {count}")
    return src.replace(old, new, 1)


def after(src, anchor, text, label):
    return once(src, anchor, anchor + text, label)


def before(src, anchor, text, label):
    return once(src, anchor, text + anchor, label)


def patch_backend(s):
    methods = '''data class ChatCommandSubmission(\n    val status: String,\n    val operationId: String?,\n    val rejectionReason: String?,\n)\n\nfun sendChatMessage(\n    conversationId: String,\n    branchId: String,\n    text: String,\n): ChatCommandSubmission = submitChatCommand(\n    commandType = "chat.send_message",\n    conversationId = conversationId,\n    branchId = branchId,\n    payload = JSONObject().put("text", text),\n    idempotent = true,\n)\n\nfun editChatMessage(\n    conversationId: String,\n    branchId: String,\n    messageId: String,\n    text: String,\n): ChatCommandSubmission = submitChatCommand(\n    commandType = "chat.edit_message",\n    conversationId = conversationId,\n    branchId = branchId,\n    payload = JSONObject().put("message_id", messageId).put("text", text),\n    idempotent = true,\n)\n\nfun regenerateChat(\n    conversationId: String,\n    branchId: String,\n): ChatCommandSubmission = submitChatCommand(\n    commandType = "recovery.regenerate",\n    conversationId = conversationId,\n    branchId = branchId,\n    payload = JSONObject(),\n    idempotent = true,\n)\n\nfun stopChatGeneration(\n    conversationId: String,\n    branchId: String,\n    operationId: String,\n): ChatCommandSubmission = submitChatCommand(\n    commandType = "chat.stop_generation",\n    conversationId = conversationId,\n    branchId = branchId,\n    payload = JSONObject(),\n    operationId = operationId,\n    idempotent = false,\n)\n\nprivate fun submitChatCommand(\n    commandType: String,\n    conversationId: String,\n    branchId: String,\n    payload: JSONObject,\n    operationId: String? = null,\n    idempotent: Boolean,\n): ChatCommandSubmission {\n    val activeGateway = gateway.get()\n        ?: return ChatCommandSubmission("rejected", null, "backend_unavailable")\n    if (conversationId.isBlank() || branchId.isBlank()) {\n        return ChatCommandSubmission("rejected", null, "stale_scope")\n    }\n    return try {\n        val envelope = JSONObject()\n            .put("protocol_version", JSONObject().put("major", 2).put("minor", 4))\n            .put("command_id", UUID.randomUUID().toString())\n            .put("request_id", UUID.randomUUID().toString())\n            .put("command_type", commandType)\n            .put("submitted_at", Instant.now().toString())\n            .put("correlation_id", UUID.randomUUID().toString())\n            .put(\n                "scope",\n                JSONObject()\n                    .put("conversation_id", conversationId)\n                    .put("branch_id", branchId),\n            )\n            .put("payload", payload)\n        if (operationId != null) envelope.put("operation_id", operationId)\n        if (idempotent) envelope.put("idempotency_key", "chat-${UUID.randomUUID()}")\n        val acknowledgement = JSONObject(\n            String(\n                activeGateway.submitCommand(\n                    envelope.toString().toByteArray(StandardCharsets.UTF_8),\n                ),\n                StandardCharsets.UTF_8,\n            ),\n        )\n        ChatCommandSubmission(\n            status = acknowledgement.optString("status", "rejected"),\n            operationId = acknowledgement.optString("operation_id").takeIf { it.isNotBlank() },\n            rejectionReason = acknowledgement.optString("rejection_reason").takeIf { it.isNotBlank() },\n        )\n    } catch (failure: Throwable) {\n        ChatCommandSubmission("rejected", null, stableFailureCode(failure))\n    }\n}\n\n'''
    s = before(s, "fun requestRuntimeSnapshot(domain: String): String? {\n", methods, "backend.chat_methods")
    s = once(
        s,
        'if (domain !in setOf("application", "settings", "protocol", "operations", "projects")) return null',
        'if (domain !in setOf("application", "settings", "protocol", "operations", "projects", "conversations")) return null',
        "backend.snapshot_domains",
    )
    return s


edit("android/app/src/main/kotlin/ai/mukei/android/BackendRuntimeHost.kt", patch_backend)


def patch_shell(s):
    s = after(s, "import java.time.LocalTime\n", "import java.util.UUID\n", "shell.uuid_import")
    s = after(
        s,
        "    var newChatGeneration by rememberSaveable { mutableIntStateOf(0) }\n",
        "    var activeConversationId by rememberSaveable { mutableStateOf<String?>(null) }\n    var activeBranchId by rememberSaveable { mutableStateOf<String?>(null) }\n    var initialChatOperationId by rememberSaveable { mutableStateOf<String?>(null) }\n",
        "shell.chat_state",
    )
    s = once(
        s,
        '''                            onClick = {\n                                selectedName = TopLevelDestination.HOME.name\n                                newChatGeneration += 1\n                            },\n''',
        '''                            onClick = {\n                                selectedName = TopLevelDestination.HOME.name\n                                activeConversationId = null\n                                activeBranchId = null\n                                initialChatOperationId = null\n                                newChatGeneration += 1\n                            },\n''',
        "shell.new_chat_reset",
    )
    s = once(
        s,
        '''                    TopLevelDestination.HOME -> HomeSurface(\n                        readiness = state.readiness,\n                        resetGeneration = newChatGeneration,\n                        openModels = { selectedName = TopLevelDestination.MODELS.name },\n                    )\n                    TopLevelDestination.STORAGE -> StorageSurface()\n                    TopLevelDestination.PROJECTS -> ProjectsSurface()\n                    TopLevelDestination.MODELS -> ModelsSurface(state.readiness)\n                    else -> ReservedDestinationSurface(selected)\n''',
        '''                    TopLevelDestination.HOME -> HomeSurface(\n                        readiness = state.readiness,\n                        resetGeneration = newChatGeneration,\n                        openModels = { selectedName = TopLevelDestination.MODELS.name },\n                        onChatStarted = { conversationId, branchId, operationId ->\n                            activeConversationId = conversationId\n                            activeBranchId = branchId\n                            initialChatOperationId = operationId\n                            selectedName = TopLevelDestination.CHATS.name\n                        },\n                    )\n                    TopLevelDestination.STORAGE -> StorageSurface()\n                    TopLevelDestination.PROJECTS -> ProjectsSurface()\n                    TopLevelDestination.MODELS -> ModelsSurface(state.readiness)\n                    TopLevelDestination.CHATS -> {\n                        val conversationId = activeConversationId\n                        val branchId = activeBranchId\n                        if (conversationId != null && branchId != null) {\n                            ChatConversationSurface(\n                                conversationId = conversationId,\n                                branchId = branchId,\n                                readiness = state.readiness,\n                                initialOperationId = initialChatOperationId,\n                                onBranchChange = { selectedBranch ->\n                                    activeBranchId = selectedBranch\n                                    initialChatOperationId = null\n                                },\n                            )\n                        } else {\n                            ChatsSurface { selectedConversation, selectedBranch ->\n                                activeConversationId = selectedConversation\n                                activeBranchId = selectedBranch\n                                initialChatOperationId = null\n                            }\n                        }\n                    }\n                    else -> ReservedDestinationSurface(selected)\n''',
        "shell.destination_chat",
    )
    s = once(
        s,
        '''private fun HomeSurface(\n    readiness: AppReadiness,\n    resetGeneration: Int,\n    openModels: () -> Unit,\n) {\n''',
        '''private fun HomeSurface(\n    readiness: AppReadiness,\n    resetGeneration: Int,\n    openModels: () -> Unit,\n    onChatStarted: (conversationId: String, branchId: String, operationId: String?) -> Unit,\n) {\n''',
        "shell.home_signature",
    )
    s = once(
        s,
        '''            MukeiComposer(\n                draft = draft,\n                onDraftChange = { draft = it },\n                placeholder = selectedCapability?.placeholder ?: "Tell Mukei what you want to do…",\n            )\n''',
        '''            MukeiComposer(\n                draft = draft,\n                onDraftChange = { draft = it },\n                placeholder = selectedCapability?.placeholder ?: "Tell Mukei what you want to do…",\n                sendEnabled = readiness.inference.status == ReadinessStatus.READY && draft.trim().isNotEmpty(),\n                onSend = {\n                    val text = draft.trim()\n                    val conversationId = UUID.randomUUID().toString()\n                    val branchId = UUID.randomUUID().toString()\n                    val result = BackendRuntimeHost.sendChatMessage(conversationId, branchId, text)\n                    if (result.status == "accepted") {\n                        draft = ""\n                        selectedCapabilityId = null\n                        onChatStarted(conversationId, branchId, result.operationId)\n                    }\n                },\n            )\n''',
        "shell.home_composer_call",
    )
    s = once(
        s,
        '''private fun MukeiComposer(\n    draft: String,\n    onDraftChange: (String) -> Unit,\n    placeholder: String,\n) {\n''',
        '''private fun MukeiComposer(\n    draft: String,\n    onDraftChange: (String) -> Unit,\n    placeholder: String,\n    sendEnabled: Boolean,\n    onSend: () -> Unit,\n) {\n''',
        "shell.composer_signature",
    )
    s = once(
        s,
        '''                    IconButton(\n                        onClick = {},\n                        enabled = false,\n                        modifier = Modifier.semantics {\n                            contentDescription = "Send unavailable until conversation runtime is connected"\n                        },\n''',
        '''                    IconButton(\n                        onClick = onSend,\n                        enabled = sendEnabled,\n                        modifier = Modifier.semantics {\n                            contentDescription = "Send message"\n                        },\n''',
        "shell.composer_send",
    )
    return s


edit("android/app/src/main/kotlin/ai/mukei/android/MukeiProductShell.kt", patch_shell)
