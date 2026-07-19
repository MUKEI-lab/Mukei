from pathlib import Path
import re


def edit(path: str, transform):
    file = Path(path)
    source = file.read_text(encoding="utf-8")
    updated = transform(source)
    if updated == source:
        raise SystemExit(f"{path}: patch produced no change")
    file.write_text(updated, encoding="utf-8")


def once(source: str, old: str, new: str, label: str) -> str:
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one anchor, found {count}")
    return source.replace(old, new, 1)


def patch_runtime(source: str) -> str:
    return once(
        source,
        'include!("application_runtime/base.rs");\ninclude!("application_runtime/chat.rs");\n',
        'include!("application_runtime/base.rs");\ninclude!("application_runtime/project_chat.rs");\ninclude!("application_runtime/chat.rs");\n',
        "runtime.project_chat_include",
    )


edit("rust/crates/mukei-core/src/application_runtime.rs", patch_runtime)


def patch_foundation_types(source: str) -> str:
    return once(
        source,
        '    AgentLoop, AgentRunRequest, ContextBudgetManager, FailureTracker, ToolExecutionPolicy,\n',
        '    AgentLoop, ContextBudgetManager, FailureTracker, ToolExecutionPolicy,\n',
        "foundation_types.remove_agent_request",
    )


edit("rust/crates/mukei-core/src/application_runtime/foundation_types.rs", patch_foundation_types)


def patch_protocol(source: str) -> str:
    source = once(
        source,
        '''pub struct SendMessagePayload {\n    /// User-authored text.\n    pub text: String,\n}\n''',
        '''pub struct SendMessagePayload {\n    /// User-authored text.\n    pub text: String,\n    /// Optional active project to bind when creating a brand-new conversation.\n    #[serde(default)]\n    pub project_id: Option<String>,\n}\n''',
        "protocol.send_payload_project",
    )
    source = once(
        source,
        '''            if !non_empty_bounded(&value.text, 64 * 1024) {\n                return Err(RejectionReason::InvalidPayload);\n            }\n            ValidatedCommandPayload::SendMessage(value)\n''',
        '''            if !non_empty_bounded(&value.text, 64 * 1024)\n                || value\n                    .project_id\n                    .as_deref()\n                    .is_some_and(|project_id| !valid_protocol_id(project_id, MAX_PROTOCOL_ID_LEN))\n            {\n                return Err(RejectionReason::InvalidPayload);\n            }\n            ValidatedCommandPayload::SendMessage(value)\n''',
        "protocol.validate_project_binding",
    )
    return source


edit("rust/crates/mukei-core/src/ui_protocol.rs", patch_protocol)


def patch_sentinel(source: str) -> str:
    source = once(
        source,
        '''    Rag,\n    ToolError,\n''',
        '''    Rag,\n    ProjectMemory,\n    ToolError,\n''',
        "sentinel.project_memory_variant",
    )
    source = once(
        source,
        '''            Self::Rag => "rag",\n            Self::ToolError => "tool_error",\n''',
        '''            Self::Rag => "rag",\n            Self::ProjectMemory => "project_memory",\n            Self::ToolError => "tool_error",\n''',
        "sentinel.project_memory_tag",
    )
    source = once(
        source,
        '''        assert_eq!(ExternalDataSource::Rag.as_str(), "rag");\n''',
        '''        assert_eq!(ExternalDataSource::Rag.as_str(), "rag");\n        assert_eq!(ExternalDataSource::ProjectMemory.as_str(), "project_memory");\n''',
        "sentinel.project_memory_test",
    )
    return source


edit("rust/crates/mukei-core/src/tools/sentinel.rs", patch_sentinel)


def patch_foundation_state(source: str) -> str:
    source = once(
        source,
        '''struct ConversationProjection {\n    conversation_id: String,\n    branch_id: String,\n    messages: Vec<ChatMessage>,\n}\n''',
        '''struct ConversationProjection {\n    conversation_id: String,\n    branch_id: String,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    project_id: Option<String>,\n    messages: Vec<ChatMessage>,\n}\n''',
        "state.conversation_projection_project",
    )
    source = once(
        source,
        '''    conversations: RwLock<HashMap<(String, String), Vec<ChatMessage>>>,\n    models: RwLock<HashMap<String, ModelProjection>>,\n''',
        '''    conversations: RwLock<HashMap<(String, String), Vec<ChatMessage>>>,\n    conversation_projects: RwLock<HashMap<String, String>>,\n    models: RwLock<HashMap<String, ModelProjection>>,\n''',
        "state.binding_field",
    )
    source = once(
        source,
        '''            conversations: RwLock::new(HashMap::new()),\n            models: RwLock::new(HashMap::new()),\n''',
        '''            conversations: RwLock::new(HashMap::new()),\n            conversation_projects: RwLock::new(HashMap::new()),\n            models: RwLock::new(HashMap::new()),\n''',
        "state.binding_init",
    )
    old_hydrate = '''        if let Some(value) = store.load("conversations").await? {\n            let records: Vec<ConversationProjection> =\n                serde_json::from_value(value).map_err(|_| MukeiError::DatabaseCorruption)?;\n            *self\n                .conversations\n                .write()\n                .unwrap_or_else(|poisoned| poisoned.into_inner()) = records\n                .into_iter()\n                .map(|record| ((record.conversation_id, record.branch_id), record.messages))\n                .collect();\n        }\n'''
    new_hydrate = '''        if let Some(value) = store.load("conversations").await? {\n            let records: Vec<ConversationProjection> =\n                serde_json::from_value(value).map_err(|_| MukeiError::DatabaseCorruption)?;\n            let mut binding_states: HashMap<String, Option<String>> = HashMap::new();\n            let mut conversations = HashMap::new();\n            for record in records {\n                if let Some(existing) = binding_states.get(&record.conversation_id) {\n                    if existing != &record.project_id {\n                        return Err(MukeiError::DatabaseCorruption);\n                    }\n                } else {\n                    binding_states\n                        .insert(record.conversation_id.clone(), record.project_id.clone());\n                }\n                conversations.insert(\n                    (record.conversation_id, record.branch_id),\n                    record.messages,\n                );\n            }\n            *self\n                .conversation_projects\n                .write()\n                .unwrap_or_else(|poisoned| poisoned.into_inner()) = binding_states\n                .into_iter()\n                .filter_map(|(conversation_id, project_id)| {\n                    project_id.map(|project_id| (conversation_id, project_id))\n                })\n                .collect();\n            *self\n                .conversations\n                .write()\n                .unwrap_or_else(|poisoned| poisoned.into_inner()) = conversations;\n        }\n'''
    source = once(source, old_hydrate, new_hydrate, "state.hydrate_bindings")
    source = once(
        source,
        '''        let records = self\n            .conversations\n            .read()\n            .unwrap_or_else(|poisoned| poisoned.into_inner())\n            .iter()\n            .map(\n                |((conversation_id, branch_id), messages)| ConversationProjection {\n                    conversation_id: conversation_id.clone(),\n                    branch_id: branch_id.clone(),\n                    messages: messages.clone(),\n                },\n            )\n            .collect::<Vec<_>>();\n''',
        '''        let bindings = self\n            .conversation_projects\n            .read()\n            .unwrap_or_else(|poisoned| poisoned.into_inner())\n            .clone();\n        let records = self\n            .conversations\n            .read()\n            .unwrap_or_else(|poisoned| poisoned.into_inner())\n            .iter()\n            .map(\n                |((conversation_id, branch_id), messages)| ConversationProjection {\n                    conversation_id: conversation_id.clone(),\n                    branch_id: branch_id.clone(),\n                    project_id: bindings.get(conversation_id).cloned(),\n                    messages: messages.clone(),\n                },\n            )\n            .collect::<Vec<_>>();\n''',
        "state.persist_bindings",
    )
    return source


edit("rust/crates/mukei-core/src/application_runtime/foundation_state.rs", patch_foundation_state)


def patch_flush(source: str) -> str:
    source = once(
        source,
        '''            let conversations = self\n                .conversations\n''',
        '''            let conversation_projects = self\n                .conversation_projects\n                .read()\n                .unwrap_or_else(|p| p.into_inner())\n                .clone();\n            let conversations = self\n                .conversations\n''',
        "flush.binding_clone",
    )
    source = once(
        source,
        '''                    |((conversation_id, branch_id), messages)| ConversationProjection {\n                        conversation_id: conversation_id.clone(),\n                        branch_id: branch_id.clone(),\n                        messages: messages.clone(),\n                    },\n''',
        '''                    |((conversation_id, branch_id), messages)| ConversationProjection {\n                        conversation_id: conversation_id.clone(),\n                        branch_id: branch_id.clone(),\n                        project_id: conversation_projects.get(conversation_id).cloned(),\n                        messages: messages.clone(),\n                    },\n''',
        "flush.binding_projection",
    )
    return source


edit("rust/crates/mukei-core/src/application_runtime/persistence_flush.rs", patch_flush)


def patch_branching(source: str) -> str:
    source = once(
        source,
        '''    fn conversations_snapshot(&self) -> Value {\n        let mut branches = self\n''',
        '''    fn conversations_snapshot(&self) -> Value {\n        let bindings = self\n            .conversation_projects\n            .read()\n            .unwrap_or_else(|poisoned| poisoned.into_inner())\n            .clone();\n        let mut branches = self\n''',
        "branching.snapshot_bindings",
    )
    source = once(
        source,
        '''                |((conversation_id, branch_id), messages)| ConversationProjection {\n                    conversation_id: conversation_id.clone(),\n                    branch_id: branch_id.clone(),\n                    messages: messages.clone(),\n                },\n''',
        '''                |((conversation_id, branch_id), messages)| ConversationProjection {\n                    conversation_id: conversation_id.clone(),\n                    branch_id: branch_id.clone(),\n                    project_id: bindings.get(conversation_id).cloned(),\n                    messages: messages.clone(),\n                },\n''',
        "branching.snapshot_project_id",
    )
    return source


edit("rust/crates/mukei-core/src/application_runtime/chat_branching.rs", patch_branching)


def patch_chat(source: str) -> str:
    old_send = '''    fn send_message(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {\n        if let Err(ack) = self.ensure_ready(command) {\n            return ack;\n        }\n        let ValidatedCommandPayload::SendMessage(payload) = &command.payload else {\n            return CommandAcknowledgementV2::rejected(\n                Some(&command.envelope),\n                RejectionReason::InvalidPayload,\n            );\n        };\n        if let Some(message_id) = command\n            .envelope\n            .scope\n            .as_ref()\n            .and_then(|scope| scope.turn_id.as_deref())\n        {\n            return self.edit_chat_message(command, message_id, &payload.text);\n        }\n        self.start_chat_operation(command, payload.text.clone(), false, None)\n    }\n'''
    new_send = '''    fn send_message(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {\n        if let Err(ack) = self.ensure_ready(command) {\n            return ack;\n        }\n        let ValidatedCommandPayload::SendMessage(payload) = &command.payload else {\n            return CommandAcknowledgementV2::rejected(\n                Some(&command.envelope),\n                RejectionReason::InvalidPayload,\n            );\n        };\n        if let Some(message_id) = command\n            .envelope\n            .scope\n            .as_ref()\n            .and_then(|scope| scope.turn_id.as_deref())\n        {\n            if payload.project_id.is_some() {\n                return CommandAcknowledgementV2::rejected(\n                    Some(&command.envelope),\n                    RejectionReason::InvalidPayload,\n                );\n            }\n            return self.edit_chat_message(command, message_id, &payload.text);\n        }\n        if let Some(project_id) = payload.project_id.as_deref() {\n            if let Err(acknowledgement) = self.ensure_inference_ready_for_branching(command) {\n                return acknowledgement;\n            }\n            let (conversation, _, _, _) = match Self::parse_chat_scope(command) {\n                Ok(value) => value,\n                Err(acknowledgement) => return acknowledgement,\n            };\n            if let Err(reason) = self\n                .features\n                .bind_conversation_project(&conversation, project_id)\n            {\n                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason);\n            }\n        }\n        self.start_chat_operation(command, payload.text.clone(), false, None)\n    }\n'''
    source = once(source, old_send, new_send, "chat.bind_on_first_send")
    source = once(
        source,
        '''        let user_message = existing_user.unwrap_or_else(|| {\n''',
        '''        let project_context = match self\n            .features\n            .project_context_message(&conversation, branch_id)\n        {\n            Ok(value) => value,\n            Err(reason) => {\n                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)\n            }\n        };\n\n        let user_message = existing_user.unwrap_or_else(|| {\n''',
        "chat.resolve_project_context",
    )
    source = once(
        source,
        '''        let (acknowledgement, operation_id, operation_token) = self.accept_operation(command);\n''',
        '''        let mut seed_history = Vec::with_capacity(2);\n        if let Some(project_context) = project_context {\n            seed_history.push(project_context);\n        }\n        seed_history.push(user_message.clone());\n        let (acknowledgement, operation_id, operation_token) = self.accept_operation(command);\n''',
        "chat.seed_project_context",
    )
    source = once(
        source,
        '''            let request = AgentRunRequest::new(\n                text,\n                conversation_id,\n                branch_id,\n                user_message_id,\n                combined_cancel,\n                token_sender,\n            );\n            let run = agent_loop.run(request);\n''',
        '''            let run = agent_loop.run_seeded(\n                seed_history,\n                conversation_id,\n                branch_id,\n                combined_cancel,\n                token_sender,\n                None,\n            );\n''',
        "chat.run_seeded_project_context",
    )
    return source


edit("rust/crates/mukei-core/src/application_runtime/chat.rs", patch_chat)


def patch_backend(source: str) -> str:
    old = '''    fun sendChatMessage(\n        conversationId: String,\n        branchId: String,\n        text: String,\n    ): ChatCommandSubmission = submitChatCommand(\n        commandType = "chat.send_message",\n        conversationId = conversationId,\n        branchId = branchId,\n        payload = JSONObject().put("text", text),\n        idempotent = true,\n    )\n'''
    new = '''    fun sendChatMessage(\n        conversationId: String,\n        branchId: String,\n        text: String,\n        projectId: String? = null,\n    ): ChatCommandSubmission {\n        val payload = JSONObject().put("text", text)\n        if (!projectId.isNullOrBlank()) payload.put("project_id", projectId)\n        return submitChatCommand(\n            commandType = "chat.send_message",\n            conversationId = conversationId,\n            branchId = branchId,\n            payload = payload,\n            idempotent = true,\n        )\n    }\n'''
    return once(source, old, new, "android.backend_project_binding")


edit("android/app/src/main/kotlin/ai/mukei/android/BackendRuntimeHost.kt", patch_backend)


def patch_chat_surface(source: str) -> str:
    source = once(
        source,
        '''private data class ChatBranchCard(\n    val conversationId: String,\n    val branchId: String,\n    val messages: List<ChatMessageCard>,\n''',
        '''private data class ChatBranchCard(\n    val conversationId: String,\n    val branchId: String,\n    val projectId: String?,\n    val messages: List<ChatMessageCard>,\n''',
        "android.branch_project_id",
    )
    source = once(
        source,
        '''    val preview: String,\n    val branchCount: Int,\n    val lastTimestamp: String,\n''',
        '''    val preview: String,\n    val branchCount: Int,\n    val projectId: String?,\n    val lastTimestamp: String,\n''',
        "android.summary_project_id",
    )
    source = once(
        source,
        '''    val summaries = remember(branches) { summarizeChats(branches) }\n''',
        '''    val summaries = remember(branches) { summarizeChats(branches) }\n    val projectNames = loadChatProjectNames()\n''',
        "android.list_project_names",
    )
    source = once(
        source,
        '''                        Spacer(Modifier.height(MukeiSpacing.Small))\n                        Text(\n                            "${chat.branchCount} branch${if (chat.branchCount == 1) "" else "es"}",\n''',
        '''                        Spacer(Modifier.height(MukeiSpacing.Small))\n                        chat.projectId?.let { projectId ->\n                            Text(\n                                "Project · ${projectNames[projectId] ?: "Bound project"}",\n                                style = MaterialTheme.typography.labelMedium,\n                                color = MaterialTheme.colorScheme.primary,\n                            )\n                            Spacer(Modifier.height(MukeiSpacing.ExtraSmall))\n                        }\n                        Text(\n                            "${chat.branchCount} branch${if (chat.branchCount == 1) "" else "es"}",\n''',
        "android.list_project_badge",
    )
    source = once(
        source,
        '''    val messages = activeBranch?.messages.orEmpty()\n    val lastAssistantId = messages.lastOrNull { it.role == "assistant" }?.messageId\n''',
        '''    val messages = activeBranch?.messages.orEmpty()\n    val activeProjectName = activeBranch?.projectId?.let { loadChatProjectNames()[it] }\n    val lastAssistantId = messages.lastOrNull { it.role == "assistant" }?.messageId\n''',
        "android.active_project_name",
    )
    source = once(
        source,
        '''    ) {\n        if (orderedBranches.size > 1 && activeIndex >= 0) {\n''',
        '''    ) {\n        activeBranch?.projectId?.let {\n            Surface(\n                modifier = Modifier.fillMaxWidth(),\n                shape = MaterialTheme.shapes.medium,\n                color = MaterialTheme.colorScheme.surfaceVariant,\n            ) {\n                Text(\n                    "Project context · ${activeProjectName ?: "Bound project"}",\n                    modifier = Modifier.padding(MukeiSpacing.Medium),\n                    style = MaterialTheme.typography.labelLarge,\n                )\n            }\n        }\n        if (orderedBranches.size > 1 && activeIndex >= 0) {\n''',
        "android.active_project_banner",
    )
    source = once(
        source,
        '''                add(ChatBranchCard(currentConversationId, branchId, messages))\n''',
        '''                add(\n                    ChatBranchCard(\n                        conversationId = currentConversationId,\n                        branchId = branchId,\n                        projectId = branch.optString("project_id").takeIf(String::isNotBlank),\n                        messages = messages,\n                    ),\n                )\n''',
        "android.parse_project_id",
    )
    source = once(
        source,
        '''            branchCount = values.size,\n            lastTimestamp = latest.lastTimestamp,\n''',
        '''            branchCount = values.size,\n            projectId = latest.projectId,\n            lastTimestamp = latest.lastTimestamp,\n''',
        "android.summary_project_value",
    )
    helper = '''\ninternal data class ChatProjectOption(\n    val projectId: String,\n    val name: String,\n)\n\ninternal fun loadActiveChatProjects(): List<ChatProjectOption> {\n    val raw = BackendRuntimeHost.requestRuntimeSnapshot("projects") ?: return emptyList()\n    return runCatching {\n        val payload = JSONObject(raw).optJSONObject("payload") ?: JSONObject()\n        val projects = payload.optJSONArray("projects") ?: JSONArray()\n        buildList {\n            for (index in 0 until projects.length()) {\n                val project = projects.optJSONObject(index) ?: continue\n                if (project.optString("status") != "active") continue\n                val projectId = project.optString("project_id")\n                val name = project.optString("name")\n                if (projectId.isBlank() || name.isBlank()) continue\n                add(ChatProjectOption(projectId, name))\n            }\n        }\n    }.getOrDefault(emptyList())\n}\n\nprivate fun loadChatProjectNames(): Map<String, String> = loadActiveChatProjects()\n    .associate { it.projectId to it.name }\n\n'''
    source = once(
        source,
        '''private fun chatFailure(code: String): String = when (code) {\n''',
        helper + '''private fun chatFailure(code: String): String = when (code) {\n''',
        "android.project_helpers",
    )
    return source


edit("android/app/src/main/kotlin/ai/mukei/android/ChatConversationSurface.kt", patch_chat_surface)
