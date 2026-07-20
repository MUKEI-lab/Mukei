from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one anchor, found {count}: {old[:120]!r}")
    file.write_text(text.replace(old, new, 1))


# Export the attachment repository/service.
replace_once(
    "rust/crates/mukei-core/src/storage/mod.rs",
    '#[cfg(feature = "rusqlite")]\npub mod conversation;\n',
    '#[cfg(feature = "rusqlite")]\npub mod conversation;\n#[cfg(feature = "rusqlite")]\npub mod conversation_attachments;\n',
)
replace_once(
    "rust/crates/mukei-core/src/storage/mod.rs",
    '#[cfg(feature = "rusqlite")]\npub use conversation::{\n    ConversationRecord, ConversationRepository, ConversationSummary, MessageRecord, MessageStatus,\n    PersistedTurn, TimelinePage, TimelineRow,\n};\n',
    '#[cfg(feature = "rusqlite")]\npub use conversation::{\n    ConversationRecord, ConversationRepository, ConversationSummary, MessageRecord, MessageStatus,\n    PersistedTurn, TimelinePage, TimelineRow,\n};\n#[cfg(feature = "rusqlite")]\npub use conversation_attachments::{\n    ConversationAttachmentContext, ConversationAttachmentPort, ConversationStorageAttachment,\n    SqlConversationAttachmentService,\n};\n',
)

# Register append-only V017 in the embedded mobile migrator.
replace_once(
    "rust/crates/mukei-core/src/storage/migrations.rs",
    '''    (
        16,
        "V016__storage_identity_and_recovery_hardening",
        include_str!("../../../../migrations/V016__storage_identity_and_recovery_hardening.sql"),
    ),
];
''',
    '''    (
        16,
        "V016__storage_identity_and_recovery_hardening",
        include_str!("../../../../migrations/V016__storage_identity_and_recovery_hardening.sql"),
    ),
    (
        17,
        "V017__conversation_storage_attachments",
        include_str!("../../../../migrations/V017__conversation_storage_attachments.sql"),
    ),
];
''',
)

# Runtime includes and service composition.
replace_once(
    "rust/crates/mukei-core/src/application_runtime.rs",
    'include!("application_runtime/conversation.rs");\n',
    'include!("application_runtime/conversation.rs");\ninclude!("application_runtime/conversation_attachments.rs");\n',
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/foundation_types.rs",
    '    pub storage_workspace: Option<Arc<dyn crate::storage::StorageWorkspacePort>>,\n',
    '    pub storage_workspace: Option<Arc<dyn crate::storage::StorageWorkspacePort>>,\n    /// Durable references from conversations to verified Universal Storage files.\n    #[cfg(feature = "rusqlite")]\n    pub conversation_attachments: Option<Arc<dyn crate::storage::ConversationAttachmentPort>>,\n',
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/foundation_runtime_struct.rs",
    '    storage_workspace: RwLock<Option<Arc<dyn crate::storage::StorageWorkspacePort>>>,\n',
    '    storage_workspace: RwLock<Option<Arc<dyn crate::storage::StorageWorkspacePort>>>,\n    #[cfg(feature = "rusqlite")]\n    conversation_attachments: RwLock<Option<Arc<dyn crate::storage::ConversationAttachmentPort>>>,\n',
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/base.rs",
    '            storage_workspace: RwLock::new(services.storage_workspace),\n',
    '            storage_workspace: RwLock::new(services.storage_workspace),\n            #[cfg(feature = "rusqlite")]\n            conversation_attachments: RwLock::new(services.conversation_attachments),\n',
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/base.rs",
    '''        #[cfg(feature = "rusqlite")]
        if self
            .storage_workspace
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
        {
            commands.extend([
                CommandType::StorageDirectoryCreate,
                CommandType::StorageNodeRename,
                CommandType::StorageNodeTrash,
                CommandType::StorageNodeRestore,
            ]);
        }
''',
    '''        #[cfg(feature = "rusqlite")]
        if self
            .storage_workspace
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
        {
            commands.extend([
                CommandType::StorageDirectoryCreate,
                CommandType::StorageNodeRename,
                CommandType::StorageNodeTrash,
                CommandType::StorageNodeRestore,
            ]);
        }
        #[cfg(feature = "rusqlite")]
        if self
            .conversation_attachments
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
        {
            commands.extend([
                CommandType::ConversationAttachmentAdd,
                CommandType::ConversationAttachmentRemove,
            ]);
        }
''',
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/foundation_context.rs",
    '            CommandType::ConversationSelectBranch => runtime.select_active_conversation_branch(command),\n',
    '            CommandType::ConversationSelectBranch => runtime.select_active_conversation_branch(command),\n            CommandType::ConversationAttachmentAdd => runtime.add_conversation_attachment(command),\n            CommandType::ConversationAttachmentRemove => runtime.remove_conversation_attachment(command),\n',
)

# Expose active attachment metadata in the existing conversations/operations snapshot.
replace_once(
    "rust/crates/mukei-core/src/application_runtime/documents_snapshot.rs",
    '''            RuntimeSnapshotDomain::Operations => {
                self.features.snapshot_with_conversations(self.platform.snapshot())
            }
''',
    '''            RuntimeSnapshotDomain::Operations => {
                let mut snapshot = self.features.snapshot_with_conversations(self.platform.snapshot());
                #[cfg(feature = "rusqlite")]
                if let Some(object) = snapshot.as_object_mut() {
                    let attachments = match self
                        .conversation_attachments
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone()
                    {
                        Some(service) => self
                            .async_runtime
                            .block_on(service.list_all())
                            .map_err(|_| RuntimeError::UnsupportedSnapshot)?,
                        None => Vec::new(),
                    };
                    object.insert(
                        "conversation_attachments".to_owned(),
                        serde_json::to_value(attachments)
                            .map_err(|_| RuntimeError::UnsupportedSnapshot)?,
                    );
                }
                snapshot
            }
''',
)

# Seed every send/edit/regenerate with conversation-level attachment context.
replace_once(
    "rust/crates/mukei-core/src/application_runtime/chat.rs",
    '''        let project_context = match self
            .features
            .project_context_message(&conversation, branch_id)
        {
            Ok(value) => value,
            Err(reason) => {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
            }
        };

        let user_message = existing_user.unwrap_or_else(|| {
''',
    '''        let project_context = match self
            .features
            .project_context_message(&conversation, branch_id)
        {
            Ok(value) => value,
            Err(reason) => {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
            }
        };
        let attachment_context = match self.attachment_context_messages(&conversation, branch_id) {
            Ok(value) => value,
            Err(reason) => {
                return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
            }
        };

        let user_message = existing_user.unwrap_or_else(|| {
''',
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/chat.rs",
    '''        let mut seed_history = Vec::with_capacity(2);
        if let Some(project_context) = project_context {
            seed_history.push(project_context);
        }
        seed_history.push(user_message.clone());
''',
    '''        let mut seed_history = Vec::with_capacity(2 + attachment_context.len());
        if let Some(project_context) = project_context {
            seed_history.push(project_context);
        }
        seed_history.extend(attachment_context);
        seed_history.push(user_message.clone());
''',
)

# Conversation deletion removes only attachment references, never Storage files.
replace_once(
    "rust/crates/mukei-core/src/application_runtime/conversation.rs",
    '''        let removed_messages = match self.features.delete_conversation_record(&conversation) {
''',
    '''        #[cfg(feature = "rusqlite")]
        {
            if self.features.conversation_record(&conversation).is_none() {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::StaleScope,
                );
            }
            if self.features.conversation_busy(&conversation) {
                return CommandAcknowledgementV2::rejected(
                    Some(&command.envelope),
                    RejectionReason::BusyConflict,
                );
            }
            if let Some(service) = self
                .conversation_attachments
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
            {
                if self
                    .async_runtime
                    .block_on(service.remove_all_for_conversation(conversation.clone()))
                    .is_err()
                {
                    return CommandAcknowledgementV2::rejected(
                        Some(&command.envelope),
                        RejectionReason::BackendUnavailable,
                    );
                }
            }
        }
        let removed_messages = match self.features.delete_conversation_record(&conversation) {
''',
)

# Protocol 2.5 attachment commands.
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    'pub const PROTOCOL_MINOR: u16 = 4;\n',
    'pub const PROTOCOL_MINOR: u16 = 5;\n',
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    '''    /// Persist the active branch selected for one conversation.
    ConversationSelectBranch,
''',
    '''    /// Persist the active branch selected for one conversation.
    ConversationSelectBranch,
    /// Attach one existing Universal Storage file to a conversation.
    ConversationAttachmentAdd,
    /// Remove one Universal Storage reference from a conversation.
    ConversationAttachmentRemove,
''',
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    '            "conversation.select_branch" => Some(Self::ConversationSelectBranch),\n',
    '            "conversation.select_branch" => Some(Self::ConversationSelectBranch),\n            "conversation.attachment.add" => Some(Self::ConversationAttachmentAdd),\n            "conversation.attachment.remove" => Some(Self::ConversationAttachmentRemove),\n',
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    '            Self::ConversationSelectBranch => "conversation.select_branch",\n',
    '            Self::ConversationSelectBranch => "conversation.select_branch",\n            Self::ConversationAttachmentAdd => "conversation.attachment.add",\n            Self::ConversationAttachmentRemove => "conversation.attachment.remove",\n',
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    '                | Self::ConversationSelectBranch\n',
    '                | Self::ConversationSelectBranch\n                | Self::ConversationAttachmentAdd\n                | Self::ConversationAttachmentRemove\n',
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    '''pub struct ConversationRenamePayload {
    /// Replacement user-visible title.
    pub title: String,
}
''',
    '''pub struct ConversationRenamePayload {
    /// Replacement user-visible title.
    pub title: String,
}

/// One Universal Storage node referenced by a conversation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationAttachmentPayload {
    /// Stable logical file node identity in Universal Storage.
    pub node_id: String,
}
''',
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    '''    /// Conversation title mutation.
    ConversationRename(ConversationRenamePayload),
''',
    '''    /// Conversation title mutation.
    ConversationRename(ConversationRenamePayload),
    /// Conversation-level Universal Storage attachment reference.
    ConversationAttachment(ConversationAttachmentPayload),
''',
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    '''                | CommandType::ConversationDelete
                | CommandType::ConversationSelectBranch
''',
    '''                | CommandType::ConversationDelete
                | CommandType::ConversationSelectBranch
                | CommandType::ConversationAttachmentAdd
                | CommandType::ConversationAttachmentRemove
''',
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    '''            CommandType::ConversationRename
            | CommandType::ConversationArchive
            | CommandType::ConversationDelete,
''',
    '''            CommandType::ConversationRename
            | CommandType::ConversationArchive
            | CommandType::ConversationDelete
            | CommandType::ConversationAttachmentAdd
            | CommandType::ConversationAttachmentRemove,
''',
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    '''        CommandType::ConversationRename => {
            let value: ConversationRenamePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.title, 128) || value.title.chars().any(char::is_control) {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ConversationRename(value)
        }
''',
    '''        CommandType::ConversationRename => {
            let value: ConversationRenamePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| RejectionReason::InvalidPayload)?;
            if !non_empty_bounded(&value.title, 128) || value.title.chars().any(char::is_control) {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ConversationRename(value)
        }
        CommandType::ConversationAttachmentAdd | CommandType::ConversationAttachmentRemove => {
            let value: ConversationAttachmentPayload =
                serde_json::from_value(envelope.payload.clone())
                    .map_err(|_| RejectionReason::InvalidPayload)?;
            if !valid_protocol_id(&value.node_id, MAX_PROTOCOL_ID_LEN) {
                return Err(RejectionReason::InvalidPayload);
            }
            ValidatedCommandPayload::ConversationAttachment(value)
        }
''',
)

# Non-SQLite builds preserve chat behavior with an empty attachment context.
replace_once(
    "rust/crates/mukei-core/src/application_runtime/conversation_attachments.rs",
    '''    fn remove_conversation_attachment(
        &self,
        command: &ValidatedCommand,
    ) -> CommandAcknowledgementV2 {
        self.add_conversation_attachment(command)
    }
}
''',
    '''    fn remove_conversation_attachment(
        &self,
        command: &ValidatedCommand,
    ) -> CommandAcknowledgementV2 {
        self.add_conversation_attachment(command)
    }

    fn attachment_context_messages(
        &self,
        _conversation_id: &str,
        _branch: BranchId,
    ) -> Result<Vec<ChatMessage>, RejectionReason> {
        Ok(Vec::new())
    }
}
''',
)

# Existing test constructor gets the new optional service slot.
replace_once(
    "rust/crates/mukei-core/src/application_runtime/storage_import_tests.rs",
    '''            RuntimeServices {
                backend_factory: None,
                storage_importer: importer,
                storage_workspace: None,
            },
''',
    '''            RuntimeServices {
                backend_factory: None,
                storage_importer: importer,
                storage_workspace: None,
                conversation_attachments: None,
            },
''',
)

# Android secure composition shares the same authenticated object store.
replace_once(
    "rust/crates/mukei-android-jni/src/secure_runtime_jni.rs",
    '''        Aes256GcmObjectCipher, DatabaseEncryptionStatus, DatabasePool, ImmutableObjectStore,
        Migrator, RuntimeProjectionRepository, SqlStorageWorkspaceService, StagedFileImporter,
        StagedPlaintextCleanup, StorageWorkspacePort, WorkspaceStagedImportService,
        DEFAULT_MAX_STAGED_IMPORT_BYTES,
''',
    '''        Aes256GcmObjectCipher, ConversationAttachmentPort, DatabaseEncryptionStatus,
        DatabasePool, ImmutableObjectStore, Migrator, RuntimeProjectionRepository,
        SqlConversationAttachmentService, SqlStorageWorkspaceService, StagedFileImporter,
        StagedPlaintextCleanup, StorageWorkspacePort, WorkspaceStagedImportService,
        DEFAULT_MAX_STAGED_IMPORT_BYTES,
''',
)
replace_once(
    "rust/crates/mukei-android-jni/src/secure_runtime_jni.rs",
    '''            let importer: Arc<dyn StagedFileImporter> = match WorkspaceStagedImportService::new(
                Arc::clone(&database_pool),
                object_store,
''',
    '''            let conversation_attachments: Arc<dyn ConversationAttachmentPort> = Arc::new(
                SqlConversationAttachmentService::new(
                    Arc::clone(&database_pool),
                    Arc::clone(&object_store),
                ),
            );
            let importer: Arc<dyn StagedFileImporter> = match WorkspaceStagedImportService::new(
                Arc::clone(&database_pool),
                Arc::clone(&object_store),
''',
)
replace_once(
    "rust/crates/mukei-android-jni/src/secure_runtime_jni.rs",
    '''            services.storage_importer = Some(importer);
            services.storage_workspace = Some(storage_workspace);
''',
    '''            services.storage_importer = Some(importer);
            services.storage_workspace = Some(storage_workspace);
            services.conversation_attachments = Some(conversation_attachments);
''',
)
replace_once(
    "rust/crates/mukei-android-jni/src/lib.rs",
    '''        backend_factory: Some(Arc::new(native_inference::AndroidLlamaBackendFactory::new(
            product.n_ctx,
            product.n_threads,
            product.gpu_layers,
            max_new_tokens,
        ))),
    }
''',
    '''        backend_factory: Some(Arc::new(native_inference::AndroidLlamaBackendFactory::new(
            product.n_ctx,
            product.n_threads,
            product.gpu_layers,
            max_new_tokens,
        ))),
        ..RuntimeServices::default()
    }
''',
)

print("conversation storage attachment backend patch applied")
