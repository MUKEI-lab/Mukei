from pathlib import Path

path = Path('.github/scripts/patch_actual_conversations.py')
text = path.read_text()
start_marker = 'path = "rust/crates/mukei-core/src/application_runtime/persistence_flush.rs"\n'
end_marker = '\npath = "rust/crates/mukei-core/src/application_runtime/chat_branching.rs"\n'
start = text.index(start_marker)
end = text.index(end_marker, start)
replacement = r'''path = "rust/crates/mukei-core/src/application_runtime/persistence_flush.rs"
replace_once(
    path,
    ''' + "'''" + r'''            let conversation_projects = self
                .conversation_projects
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            let conversations = self
                .conversations
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .map(
                    |((conversation_id, branch_id), messages)| ConversationProjection {
                        conversation_id: conversation_id.clone(),
                        branch_id: branch_id.clone(),
                        project_id: conversation_projects.get(conversation_id).cloned(),
                        messages: messages.clone(),
                    },
                )
                .collect::<Vec<_>>();
''' + "'''" + r''',
    ''' + "'''" + r'''            let conversation_projects = self
                .conversation_projects
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            let conversations = self
                .conversations
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .map(
                    |((conversation_id, branch_id), messages)| ConversationProjection {
                        conversation_id: conversation_id.clone(),
                        branch_id: branch_id.clone(),
                        project_id: conversation_projects.get(conversation_id).cloned(),
                        messages: messages.clone(),
                    },
                )
                .collect::<Vec<_>>();
            let conversation_metadata = self
                .conversation_records
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
''' + "'''" + r''',
)
replace_once(
    path,
    ''' + "'''" + r'''                (
                    "projects",
                    serde_json::to_value(projects)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
''' + "'''" + r''',
    ''' + "'''" + r'''                (
                    "conversation_metadata",
                    serde_json::to_value(conversation_metadata)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
                (
                    "projects",
                    serde_json::to_value(projects)
                        .map_err(|error| MukeiError::Internal(error.to_string()))?,
                ),
''' + "'''" + r''',
)
'''
path.write_text(text[:start] + replacement + text[end:])
print('retargeted persistence_flush patch anchor')
