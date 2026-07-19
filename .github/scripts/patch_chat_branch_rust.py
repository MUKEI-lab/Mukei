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


edit(
    "rust/crates/mukei-core/src/application_runtime.rs",
    lambda s: after(
        s,
        'include!("application_runtime/chat.rs");\n',
        'include!("application_runtime/chat_branching.rs");\n',
        "runtime.include_branching",
    ),
)


def patch_foundation_types(s):
    s = after(s, "    Projects,\n", "    Conversations,\n", "snapshot.runtime.variant")
    s = after(
        s,
        '            "projects" => Some(Self::Projects),\n',
        '            "conversations" => Some(Self::Conversations),\n',
        "snapshot.runtime.parse",
    )
    return s


edit("rust/crates/mukei-core/src/application_runtime/foundation_types.rs", patch_foundation_types)

edit(
    "rust/crates/mukei-core/src/application_runtime/documents_snapshot.rs",
    lambda s: after(
        s,
        "            RuntimeSnapshotDomain::Projects => self.features.projects_snapshot(),\n",
        "            RuntimeSnapshotDomain::Conversations => self.features.conversations_snapshot(),\n",
        "snapshot.runtime.payload",
    ),
)


def patch_protocol(s):
    s = once(s, "pub const PROTOCOL_MINOR: u16 = 3;", "pub const PROTOCOL_MINOR: u16 = 4;", "protocol.minor")
    s = after(
        s,
        "    ChatClearConversation,\n",
        "    /// Edit a user or assistant message by forking immutable history.\n    ChatEditMessage,\n",
        "protocol.command.variant",
    )
    s = after(
        s,
        '            "chat.clear_conversation" => Some(Self::ChatClearConversation),\n',
        '            "chat.edit_message" => Some(Self::ChatEditMessage),\n',
        "protocol.command.parse",
    )
    s = after(
        s,
        '            Self::ChatClearConversation => "chat.clear_conversation",\n',
        '            Self::ChatEditMessage => "chat.edit_message",\n',
        "protocol.command.as_str",
    )
    s = after(s, "            Self::ChatSendMessage\n", "                | Self::ChatEditMessage\n", "protocol.command.idempotency")
    payload = '''\n/// Edit one durable chat message by creating a new branch.\n#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\npub struct EditMessagePayload {\n    /// Stable message identity in the source branch.\n    pub message_id: String,\n    /// Replacement user-authored content.\n    pub text: String,\n}\n'''
    s = after(
        s,
        'pub struct SendMessagePayload {\n    /// User-authored text.\n    pub text: String,\n}\n',
        payload,
        "protocol.edit_payload",
    )
    s = after(
        s,
        "    SendMessage(SendMessagePayload),\n",
        "    /// Edited message content.\n    EditMessage(EditMessagePayload),\n",
        "protocol.validated_payload",
    )
    # This anchor occurs in SnapshotDomainV2 only after the command enum has already been patched.
    snapshot_anchor = "    /// Durable encrypted project records.\n    Projects,\n"
    s = after(s, snapshot_anchor, "    /// Durable conversation branches and messages.\n    Conversations,\n", "protocol.snapshot.variant")
    s = after(
        s,
        '            "projects" => Some(Self::Projects),\n',
        '            "conversations" => Some(Self::Conversations),\n',
        "protocol.snapshot.parse",
    )
    s = once(
        s,
        "            CommandType::RecoveryResume\n                | CommandType::RecoveryRegenerate\n                | CommandType::StorageImportFile\n",
        "            CommandType::RecoveryResume\n                | CommandType::RecoveryRegenerate\n                | CommandType::ChatEditMessage\n                | CommandType::StorageImportFile\n",
        "protocol.scope.required",
    )
    scope_case = '''        (CommandType::ChatEditMessage, ValidatedCommandPayload::EditMessage(_)) => {\n            if scope.conversation_id.is_none()\n                || scope.branch_id.is_none()\n                || has_model\n                || has_document\n            {\n                return Err(RejectionReason::StaleScope);\n            }\n        }\n'''
    s = before(
        s,
        "        (CommandType::ChatSendMessage | CommandType::ChatClearConversation, _) => {\n",
        scope_case,
        "protocol.scope.edit",
    )
    validation = '''        CommandType::ChatEditMessage => {\n            let value: EditMessagePayload = serde_json::from_value(envelope.payload.clone())\n                .map_err(|_| RejectionReason::InvalidPayload)?;\n            if !valid_protocol_id(&value.message_id, MAX_PROTOCOL_ID_LEN)\n                || !non_empty_bounded(&value.text, 64 * 1024)\n            {\n                return Err(RejectionReason::InvalidPayload);\n            }\n            ValidatedCommandPayload::EditMessage(value)\n        }\n'''
    s = before(s, "        CommandType::ModelDownload => {\n", validation, "protocol.validation.edit")
    return s


edit("rust/crates/mukei-core/src/ui_protocol.rs", patch_protocol)

edit(
    "rust/crates/mukei-core/src/application_runtime/base.rs",
    lambda s: after(
        s,
        "                CommandType::ChatStopGeneration,\n",
        "                CommandType::ChatEditMessage,\n",
        "base.capabilities.chat_edit",
    ),
)

edit(
    "rust/crates/mukei-core/src/application_runtime/foundation_context.rs",
    lambda s: after(
        s,
        "            CommandType::ChatClearConversation => runtime.clear_conversation(command),\n",
        "            CommandType::ChatEditMessage => runtime.edit_chat_message(command),\n",
        "router.chat_edit",
    ),
)


def patch_recovery(s):
    start = s.index("    fn recover_chat(")
    end_marker = "\n    }\n}"
    end = s.index(end_marker, start) + len("\n    }")
    replacement = '''    fn recover_chat(&self, command: &ValidatedCommand, regenerate: bool) -> CommandAcknowledgementV2 {\n        if regenerate {\n            return self.regenerate_chat_branch(command);\n        }\n        if let Err(ack) = self.ensure_ready(command) {\n            return ack;\n        }\n        let (conversation, branch, _, _) = match Self::parse_chat_scope(command) {\n            Ok(value) => value,\n            Err(ack) => return ack,\n        };\n        let Some(user_message) = self.features.last_user_message(&conversation, &branch) else {\n            return CommandAcknowledgementV2::rejected(\n                Some(&command.envelope),\n                RejectionReason::StaleScope,\n            );\n        };\n        self.start_chat_operation(\n            command,\n            user_message.content.clone(),\n            false,\n            Some(user_message),\n        )\n    }'''
    return s[:start] + replacement + s[end:]


edit("rust/crates/mukei-core/src/application_runtime/documents_retry_settings.rs", patch_recovery)


def patch_chat(s):
    old = '''        let user_message = existing_user.unwrap_or_else(|| {\n            ChatMessage::user_with_id(MessageId::new(), branch_id, text.clone())\n        });\n'''
    new = '''        let user_message = existing_user.unwrap_or_else(|| {\n            let mut message = ChatMessage::user_with_id(MessageId::new(), branch_id, text.clone());\n            message.parent = self\n                .features\n                .history(conversation_id, branch_id)\n                .last()\n                .map(|value| value.id);\n            message\n        });\n'''
    return once(s, old, new, "chat.user_parent")


edit("rust/crates/mukei-core/src/application_runtime/chat.rs", patch_chat)
