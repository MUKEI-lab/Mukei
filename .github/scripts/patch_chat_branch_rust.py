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


def patch_recovery(s):
    start = s.index("    fn recover_chat(")
    end = s.index("\n    }\n}", start) + len("\n    }")
    replacement = '''    fn recover_chat(&self, command: &ValidatedCommand, regenerate: bool) -> CommandAcknowledgementV2 {\n        if regenerate {\n            return self.regenerate_chat_branch(command);\n        }\n        if let Err(ack) = self.ensure_ready(command) {\n            return ack;\n        }\n        let (conversation, branch, _, _) = match Self::parse_chat_scope(command) {\n            Ok(value) => value,\n            Err(ack) => return ack,\n        };\n        let Some(user_message) = self.features.last_user_message(&conversation, &branch) else {\n            return CommandAcknowledgementV2::rejected(\n                Some(&command.envelope),\n                RejectionReason::StaleScope,\n            );\n        };\n        self.start_chat_operation(\n            command,\n            user_message.content.clone(),\n            false,\n            Some(user_message),\n        )\n    }'''
    return s[:start] + replacement + s[end:]


edit("rust/crates/mukei-core/src/application_runtime/documents_retry_settings.rs", patch_recovery)


def patch_chat(s):
    old_send = '''        let ValidatedCommandPayload::SendMessage(payload) = &command.payload else {\n            return CommandAcknowledgementV2::rejected(\n                Some(&command.envelope),\n                RejectionReason::InvalidPayload,\n            );\n        };\n        self.start_chat_operation(command, payload.text.clone(), false, None)\n'''
    new_send = '''        let ValidatedCommandPayload::SendMessage(payload) = &command.payload else {\n            return CommandAcknowledgementV2::rejected(\n                Some(&command.envelope),\n                RejectionReason::InvalidPayload,\n            );\n        };\n        if let Some(message_id) = command\n            .envelope\n            .scope\n            .as_ref()\n            .and_then(|scope| scope.turn_id.as_deref())\n        {\n            return self.edit_chat_message(command, message_id, &payload.text);\n        }\n        self.start_chat_operation(command, payload.text.clone(), false, None)\n'''
    s = once(s, old_send, new_send, "chat.edit_routing")
    old_user = '''        let user_message = existing_user.unwrap_or_else(|| {\n            ChatMessage::user_with_id(MessageId::new(), branch_id, text.clone())\n        });\n'''
    new_user = '''        let user_message = existing_user.unwrap_or_else(|| {\n            let mut message = ChatMessage::user_with_id(MessageId::new(), branch_id, text.clone());\n            message.parent = self\n                .features\n                .history(conversation_id, branch_id)\n                .last()\n                .map(|value| value.id);\n            message\n        });\n'''
    return once(s, old_user, new_user, "chat.user_parent")


edit("rust/crates/mukei-core/src/application_runtime/chat.rs", patch_chat)
