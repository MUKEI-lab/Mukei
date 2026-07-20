from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one anchor, found {count}: {old[:100]!r}")
    p.write_text(text.replace(old, new, 1))

# storage module exports
replace_once(
    "rust/crates/mukei-core/src/storage/mod.rs",
    "#[cfg(feature = \"rusqlite\")]\npub mod version_repository;\n",
    "#[cfg(feature = \"rusqlite\")]\npub mod version_repository;\n#[cfg(feature = \"rusqlite\")]\npub mod workspace_service;\n",
)
replace_once(
    "rust/crates/mukei-core/src/storage/mod.rs",
    "#[cfg(feature = \"rusqlite\")]\npub use version_repository::{\n    FileVersionRepository, NewFileVersion, PersistedFileVersion, VersionCreator,\n};\n",
    "#[cfg(feature = \"rusqlite\")]\npub use version_repository::{\n    FileVersionRepository, NewFileVersion, PersistedFileVersion, VersionCreator,\n};\n#[cfg(feature = \"rusqlite\")]\npub use workspace_service::{\n    SqlStorageWorkspaceService, StorageNodeSnapshot, StorageWorkspacePort,\n    UniversalStorageSnapshot,\n};\n",
)

# universal staged import
replace_once(
    "rust/crates/mukei-core/src/storage/staged_import.rs",
    "use crate::storage::pool::DatabasePool;\n",
    "use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};\n",
)
replace_once(
    "rust/crates/mukei-core/src/storage/staged_import.rs",
    "    ChatId, DuplicatePolicy, ImportTransactionId, StorageNodeId, StorageObjectId,\n    WorkspaceAccessContext,\n",
    "    ChatId, DuplicatePolicy, ImportTransactionId, StorageNodeId, StorageObjectId,\n    StorageScopeId, WorkspaceAccessContext,\n",
)
replace_once(
    "rust/crates/mukei-core/src/storage/staged_import.rs",
    "pub struct WorkspaceStagedImportRequest {\n    pub chat_id: ChatId,\n    pub staged_path: PathBuf,\n    pub original_filename: String,\n    pub detected_mime: Option<String>,\n    pub expected_size: Option<u64>,\n    pub duplicate_policy: DuplicatePolicy,\n    pub source_uri_fingerprint: Option<String>,\n}\n",
    "pub struct WorkspaceStagedImportRequest {\n    pub chat_id: ChatId,\n    pub staged_path: PathBuf,\n    pub original_filename: String,\n    pub detected_mime: Option<String>,\n    pub expected_size: Option<u64>,\n    pub duplicate_policy: DuplicatePolicy,\n    pub source_uri_fingerprint: Option<String>,\n}\n\n/// One Android-staged file destined for an explicit Universal Storage directory.\n#[derive(Clone, Debug)]\npub struct UniversalStagedImportRequest {\n    pub parent_node_id: StorageNodeId,\n    pub staged_path: PathBuf,\n    pub original_filename: String,\n    pub detected_mime: Option<String>,\n    pub expected_size: Option<u64>,\n    pub duplicate_policy: DuplicatePolicy,\n    pub source_uri_fingerprint: Option<String>,\n}\n",
)
replace_once(
    "rust/crates/mukei-core/src/storage/staged_import.rs",
    "pub trait StagedFileImporter: Send + Sync {\n    async fn import_workspace_file(\n        &self,\n        request: WorkspaceStagedImportRequest,\n        cancellation: CancellationToken,\n    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError>;\n}\n",
    "pub trait StagedFileImporter: Send + Sync {\n    async fn import_workspace_file(\n        &self,\n        request: WorkspaceStagedImportRequest,\n        cancellation: CancellationToken,\n    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError>;\n\n    async fn import_universal_file(\n        &self,\n        _request: UniversalStagedImportRequest,\n        _cancellation: CancellationToken,\n    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError> {\n        Err(StagedImportError::InvalidConfiguration)\n    }\n}\n",
)
anchor = """        Ok(WorkspaceStagedImportReceipt {\n            transaction_id,\n            node_id: receipt.node_id,\n            object_id: receipt.object_id,\n            display_name: receipt.display_name,\n            plaintext_size,\n            deduplicated,\n            staged_file_removed,\n        })\n    }\n}\n\n#[async_trait::async_trait]\nimpl<C> StagedFileImporter for WorkspaceStagedImportService<C>\n"""
insert = """        Ok(WorkspaceStagedImportReceipt {\n            transaction_id,\n            node_id: receipt.node_id,\n            object_id: receipt.object_id,\n            display_name: receipt.display_name,\n            plaintext_size,\n            deduplicated,\n            staged_file_removed,\n        })\n    }\n\n    async fn execute_universal_import(\n        &self,\n        transaction_id: ImportTransactionId,\n        request: UniversalStagedImportRequest,\n        admitted_name: crate::storage::file_policy::AllowedFileName,\n        canonical_path: PathBuf,\n        cancellation: CancellationToken,\n    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError>\n    where\n        C: Send + Sync + 'static,\n    {\n        transition(&self.pool, transaction_id, ImportState::Validating).await?;\n        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;\n        transition(&self.pool, transaction_id, ImportState::Copying).await?;\n        let path_for_read = canonical_path.clone();\n        let maximum = self.max_import_bytes;\n        let expected_size = request.expected_size;\n        let bytes = tokio::task::spawn_blocking(move || {\n            read_bounded_staged_file(&path_for_read, maximum, expected_size)\n        })\n        .await\n        .map_err(|error| StagedImportError::BlockingTask(error.to_string()))??;\n        ImportJournalRepository::record_progress(&self.pool, transaction_id, bytes.len() as u64)\n            .await?;\n        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;\n        transition(&self.pool, transaction_id, ImportState::Hashing).await?;\n        validate_text_content(&bytes)?;\n        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;\n        transition(&self.pool, transaction_id, ImportState::Encrypting).await?;\n        cancel_if_requested(&self.pool, transaction_id, &cancellation).await?;\n        let object_store = Arc::clone(&self.object_store);\n        let stored_object = tokio::task::spawn_blocking(move || object_store.put(&bytes))\n            .await\n            .map_err(|error| StagedImportError::BlockingTask(error.to_string()))??;\n        transition(&self.pool, transaction_id, ImportState::CommittingObject).await?;\n        transition(&self.pool, transaction_id, ImportState::CommittingNode).await?;\n        let plaintext_size = stored_object.plaintext_size;\n        let deduplicated = stored_object.deduplicated;\n        let detected_format = match &admitted_name.rule {\n            FileAdmissionRule::Extension(extension) => extension.to_string(),\n            FileAdmissionRule::ExactName(name) => format!(\"exact:{name}\"),\n        };\n        let receipt = ImportCommitRepository::commit(\n            &self.pool,\n            ImportCommitRequest {\n                transaction_id,\n                authorization: ImportAuthorization::Universal,\n                admitted_name,\n                stored_object,\n                detected_format,\n                detected_mime: request\n                    .detected_mime\n                    .filter(|value| !value.trim().is_empty()),\n                detected_encoding: Some(\"utf-8\".to_string()),\n                language_id: None,\n                encryption_version: self.object_store.encryption_version(),\n                duplicate_policy: request.duplicate_policy,\n            },\n        )\n        .await?;\n        transition(&self.pool, transaction_id, ImportState::Completed).await?;\n        let cleanup_path = canonical_path;\n        let staged_file_removed =\n            tokio::task::spawn_blocking(move || fs::remove_file(cleanup_path).is_ok())\n                .await\n                .unwrap_or(false);\n        Ok(WorkspaceStagedImportReceipt {\n            transaction_id,\n            node_id: receipt.node_id,\n            object_id: receipt.object_id,\n            display_name: receipt.display_name,\n            plaintext_size,\n            deduplicated,\n            staged_file_removed,\n        })\n    }\n}\n\n#[async_trait::async_trait]\nimpl<C> StagedFileImporter for WorkspaceStagedImportService<C>\n"""
replace_once("rust/crates/mukei-core/src/storage/staged_import.rs", anchor, insert)
anchor = """        result\n    }\n}\n\nstruct InspectedStagedFile {\n"""
insert = """        result\n    }\n\n    async fn import_universal_file(\n        &self,\n        request: UniversalStagedImportRequest,\n        cancellation: CancellationToken,\n    ) -> Result<WorkspaceStagedImportReceipt, StagedImportError> {\n        let admitted_name = admit_file_name(&request.original_filename)?;\n        if request.duplicate_policy == DuplicatePolicy::ReplaceWithNewVersion {\n            return Err(StagedImportError::InvalidConfiguration);\n        }\n        let inspected = inspect_staged_file(\n            &self.staging_root,\n            &request.staged_path,\n            self.max_import_bytes,\n            request.expected_size,\n        )?;\n        let universal = UniversalStorageRepository::ensure_universal_storage(&self.pool).await?;\n        validate_universal_import_parent(&self.pool, universal.scope_id, request.parent_node_id)\n            .await?;\n        let transaction_id = ImportJournalRepository::create(\n            &self.pool,\n            universal.scope_id,\n            request.parent_node_id,\n            admitted_name.display_name.clone(),\n            inspected.relative_path,\n            Some(inspected.size),\n            request.source_uri_fingerprint.clone(),\n        )\n        .await?;\n        let result = self\n            .execute_universal_import(\n                transaction_id,\n                request,\n                admitted_name,\n                inspected.canonical_path,\n                cancellation,\n            )\n            .await;\n        if let Err(error) = &result {\n            if !matches!(error, StagedImportError::Cancelled) {\n                let _ = ImportJournalRepository::transition(\n                    &self.pool,\n                    transaction_id,\n                    ImportState::Failed,\n                    Some(error.code().to_string()),\n                    None,\n                )\n                .await;\n            }\n        }\n        result\n    }\n}\n\nasync fn validate_universal_import_parent(\n    pool: &DatabasePool,\n    scope_id: StorageScopeId,\n    parent_node_id: StorageNodeId,\n) -> Result<(), StagedImportError> {\n    pool.with_conn(move |connection| {\n        let valid: bool = connection.query_row(\n            \"SELECT EXISTS(SELECT 1 FROM storage_nodes n \\\n             JOIN storage_scopes s ON s.scope_id = n.scope_id \\\n             WHERE n.node_id = ?1 AND n.scope_id = ?2 AND n.node_type = 'directory' \\\n               AND n.state = 'active' AND s.scope_type = 'universal' AND s.state = 'active' \\\n               AND COALESCE(n.system_role, '') != 'trash')\",\n            rusqlite::params![parent_node_id.to_string(), scope_id.to_string()],\n            |row| row.get::<_, i64>(0).map(|value| value != 0),\n        )?;\n        if !valid {\n            return Err(DbError::Domain(MukeiError::Invariant(\n                \"universal import parent is not an active user-writable directory\".into(),\n            )));\n        }\n        Ok::<_, DbError>(())\n    })\n    .await?;\n    Ok(())\n}\n\nstruct InspectedStagedFile {\n"""
replace_once("rust/crates/mukei-core/src/storage/staged_import.rs", anchor, insert)

# export universal request
replace_once(
    "rust/crates/mukei-core/src/storage/mod.rs",
    "    StagedFileImporter, StagedImportError, WorkspaceStagedImportReceipt,\n    WorkspaceStagedImportRequest, WorkspaceStagedImportService, DEFAULT_MAX_STAGED_IMPORT_BYTES,\n",
    "    StagedFileImporter, StagedImportError, UniversalStagedImportRequest,\n    WorkspaceStagedImportReceipt, WorkspaceStagedImportRequest, WorkspaceStagedImportService,\n    DEFAULT_MAX_STAGED_IMPORT_BYTES,\n",
)

# Runtime services and storage snapshot domain
replace_once(
    "rust/crates/mukei-core/src/application_runtime/foundation_types.rs",
    "    pub storage_importer: Option<Arc<dyn crate::storage::StagedFileImporter>>,\n",
    "    pub storage_importer: Option<Arc<dyn crate::storage::StagedFileImporter>>,\n    /// SQLCipher-backed logical Universal Storage workspace service.\n    #[cfg(feature = \"rusqlite\")]\n    pub storage_workspace: Option<Arc<dyn crate::storage::StorageWorkspacePort>>,\n",
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/foundation_types.rs",
    "    Operations,\n    Projects,\n",
    "    Operations,\n    Projects,\n    Storage,\n",
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/foundation_types.rs",
    "            \"projects\" => Some(Self::Projects),\n",
    "            \"projects\" => Some(Self::Projects),\n            \"storage\" => Some(Self::Storage),\n",
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/foundation_runtime_struct.rs",
    "    storage_importer: RwLock<Option<Arc<dyn crate::storage::StagedFileImporter>>>,\n",
    "    storage_importer: RwLock<Option<Arc<dyn crate::storage::StagedFileImporter>>>,\n    #[cfg(feature = \"rusqlite\")]\n    storage_workspace: RwLock<Option<Arc<dyn crate::storage::StorageWorkspacePort>>>,\n",
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/base.rs",
    "            storage_importer: RwLock::new(services.storage_importer),\n",
    "            storage_importer: RwLock::new(services.storage_importer),\n            #[cfg(feature = \"rusqlite\")]\n            storage_workspace: RwLock::new(services.storage_workspace),\n",
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/base.rs",
    "            commands.push(CommandType::StorageImportFile);\n        }\n",
    "            commands.push(CommandType::StorageImportFile);\n        }\n        #[cfg(feature = \"rusqlite\")]\n        if self\n            .storage_workspace\n            .read()\n            .unwrap_or_else(|poisoned| poisoned.into_inner())\n            .is_some()\n        {\n            commands.extend([\n                CommandType::StorageDirectoryCreate,\n                CommandType::StorageNodeRename,\n                CommandType::StorageNodeTrash,\n                CommandType::StorageNodeRestore,\n            ]);\n        }\n",
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/documents_snapshot.rs",
    "            RuntimeSnapshotDomain::Projects => self.features.projects_snapshot(),\n",
    "            RuntimeSnapshotDomain::Projects => self.features.projects_snapshot(),\n            RuntimeSnapshotDomain::Storage => {\n                #[cfg(feature = \"rusqlite\")]\n                {\n                    let service = self\n                        .storage_workspace\n                        .read()\n                        .unwrap_or_else(|poisoned| poisoned.into_inner())\n                        .clone()\n                        .ok_or(RuntimeError::UnsupportedSnapshot)?;\n                    let snapshot = self\n                        .async_runtime\n                        .block_on(service.universal_snapshot())\n                        .map_err(|_| RuntimeError::UnsupportedSnapshot)?;\n                    serde_json::to_value(snapshot)\n                        .map_err(|_| RuntimeError::UnsupportedSnapshot)?\n                }\n                #[cfg(not(feature = \"rusqlite\"))]\n                {\n                    return Err(RuntimeError::UnsupportedSnapshot);\n                }\n            }\n",
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime.rs",
    "include!(\"application_runtime/storage_import.rs\");\n",
    "include!(\"application_runtime/storage_import.rs\");\ninclude!(\"application_runtime/storage_workspace.rs\");\n",
)

# Protocol 2.4: commands and payloads
replace_once("rust/crates/mukei-core/src/ui_protocol.rs", "pub const PROTOCOL_MINOR: u16 = 3;", "pub const PROTOCOL_MINOR: u16 = 4;")
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "    /// Import a selected Android document into the active chat workspace.\n    StorageImportFile,\n",
    "    /// Import a selected Android document into a chat workspace or Universal Storage.\n    StorageImportFile,\n    /// Create a user-owned Universal Storage directory.\n    StorageDirectoryCreate,\n    /// Rename a user-owned Universal Storage node.\n    StorageNodeRename,\n    /// Move a user-owned Universal Storage node into Trash.\n    StorageNodeTrash,\n    /// Restore a trashed Universal Storage node.\n    StorageNodeRestore,\n",
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "            \"storage.import_file\" => Some(Self::StorageImportFile),\n",
    "            \"storage.import_file\" => Some(Self::StorageImportFile),\n            \"storage.directory.create\" => Some(Self::StorageDirectoryCreate),\n            \"storage.node.rename\" => Some(Self::StorageNodeRename),\n            \"storage.node.trash\" => Some(Self::StorageNodeTrash),\n            \"storage.node.restore\" => Some(Self::StorageNodeRestore),\n",
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "            Self::StorageImportFile => \"storage.import_file\",\n",
    "            Self::StorageImportFile => \"storage.import_file\",\n            Self::StorageDirectoryCreate => \"storage.directory.create\",\n            Self::StorageNodeRename => \"storage.node.rename\",\n            Self::StorageNodeTrash => \"storage.node.trash\",\n            Self::StorageNodeRestore => \"storage.node.restore\",\n",
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "                | Self::StorageImportFile\n",
    "                | Self::StorageImportFile\n                | Self::StorageDirectoryCreate\n                | Self::StorageNodeRename\n                | Self::StorageNodeTrash\n                | Self::StorageNodeRestore\n",
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "pub struct StorageImportPayload {\n    /// Opaque `content://` URI handled only by the Android document port.\n    pub target: String,\n    /// User-visible filename validated again by storage admission policy.\n    pub display_name: String,\n    /// MIME type reported by Android.\n    pub mime_type: String,\n}\n",
    "pub struct StorageImportPayload {\n    /// Opaque `content://` URI handled only by the Android document port.\n    pub target: String,\n    /// User-visible filename validated again by storage admission policy.\n    pub display_name: String,\n    /// MIME type reported by Android.\n    pub mime_type: String,\n    /// Explicit Universal Storage destination. Absence preserves chat-workspace import semantics.\n    #[serde(default)]\n    pub parent_node_id: Option<String>,\n}\n\n/// New Universal Storage directory payload.\n#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\npub struct StorageDirectoryCreatePayload {\n    pub parent_node_id: String,\n    pub name: String,\n}\n\n/// Rename one user-owned Universal Storage node.\n#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\npub struct StorageNodeRenamePayload {\n    pub node_id: String,\n    pub name: String,\n}\n\n/// Identity of one Universal Storage node.\n#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\npub struct StorageNodePayload {\n    pub node_id: String,\n}\n",
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "    /// Selected document destined for encrypted workspace storage.\n    StorageImport(StorageImportPayload),\n",
    "    /// Selected document destined for encrypted workspace storage.\n    StorageImport(StorageImportPayload),\n    /// New Universal Storage directory.\n    StorageDirectoryCreate(StorageDirectoryCreatePayload),\n    /// Universal Storage node rename.\n    StorageNodeRename(StorageNodeRenamePayload),\n    /// Universal Storage node identity.\n    StorageNode(StorageNodePayload),\n",
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "    /// Durable encrypted project records.\n    Projects,\n",
    "    /// Durable encrypted project records.\n    Projects,\n    /// Authoritative Universal Storage tree.\n    Storage,\n",
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "            \"projects\" => Some(Self::Projects),\n",
    "            \"projects\" => Some(Self::Projects),\n            \"storage\" => Some(Self::Storage),\n",
)
# storage scope semantics: workspace import has conversation; universal import has parent and no scope
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "        (CommandType::StorageImportFile, ValidatedCommandPayload::StorageImport(_)) => {\n            if scope.conversation_id.is_none() || has_model || has_document {\n                return Err(RejectionReason::StaleScope);\n            }\n        }\n",
    "        (CommandType::StorageImportFile, ValidatedCommandPayload::StorageImport(value)) => {\n            let universal = value.parent_node_id.is_some();\n            if has_model\n                || has_document\n                || (universal && has_conversation)\n                || (!universal && scope.conversation_id.is_none())\n            {\n                return Err(RejectionReason::StaleScope);\n            }\n        }\n",
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "             | CommandType::SettingsUpdate,\n",
    "             | CommandType::StorageDirectoryCreate\n             | CommandType::StorageNodeRename\n             | CommandType::StorageNodeTrash\n             | CommandType::StorageNodeRestore\n             | CommandType::SettingsUpdate,\n",
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "                || !non_empty_bounded(&value.mime_type, 256)\n            {\n",
    "                || !non_empty_bounded(&value.mime_type, 256)\n                || value.parent_node_id.as_deref().is_some_and(|node_id| {\n                    !valid_protocol_id(node_id, MAX_PROTOCOL_ID_LEN)\n                })\n            {\n",
)
replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "            ValidatedCommandPayload::StorageImport(value)\n        }\n        CommandType::DocumentRevoke",
    "            ValidatedCommandPayload::StorageImport(value)\n        }\n        CommandType::StorageDirectoryCreate => {\n            let value: StorageDirectoryCreatePayload = serde_json::from_value(envelope.payload.clone())\n                .map_err(|_| RejectionReason::InvalidPayload)?;\n            if !valid_protocol_id(&value.parent_node_id, MAX_PROTOCOL_ID_LEN)\n                || !non_empty_bounded(&value.name, 255)\n                || value.name.chars().any(char::is_control)\n            {\n                return Err(RejectionReason::InvalidPayload);\n            }\n            ValidatedCommandPayload::StorageDirectoryCreate(value)\n        }\n        CommandType::StorageNodeRename => {\n            let value: StorageNodeRenamePayload = serde_json::from_value(envelope.payload.clone())\n                .map_err(|_| RejectionReason::InvalidPayload)?;\n            if !valid_protocol_id(&value.node_id, MAX_PROTOCOL_ID_LEN)\n                || !non_empty_bounded(&value.name, 255)\n                || value.name.chars().any(char::is_control)\n            {\n                return Err(RejectionReason::InvalidPayload);\n            }\n            ValidatedCommandPayload::StorageNodeRename(value)\n        }\n        CommandType::StorageNodeTrash | CommandType::StorageNodeRestore => {\n            let value: StorageNodePayload = serde_json::from_value(envelope.payload.clone())\n                .map_err(|_| RejectionReason::InvalidPayload)?;\n            if !valid_protocol_id(&value.node_id, MAX_PROTOCOL_ID_LEN) {\n                return Err(RejectionReason::InvalidPayload);\n            }\n            ValidatedCommandPayload::StorageNode(value)\n        }\n        CommandType::DocumentRevoke",
)

# Router
replace_once(
    "rust/crates/mukei-core/src/application_runtime/foundation_context.rs",
    "            CommandType::StorageImportFile => runtime.import_storage_file(command),\n",
    "            CommandType::StorageImportFile => runtime.import_storage_file(command),\n            CommandType::StorageDirectoryCreate => runtime.create_storage_directory(command),\n            CommandType::StorageNodeRename => runtime.rename_storage_node(command),\n            CommandType::StorageNodeTrash => runtime.trash_storage_node(command),\n            CommandType::StorageNodeRestore => runtime.restore_storage_node(command),\n",
)

# Parent-aware storage import runtime
replace_once(
    "rust/crates/mukei-core/src/application_runtime/storage_import.rs",
    "        let Some(conversation_id) = command\n            .envelope\n            .scope\n            .as_ref()\n            .and_then(|scope| scope.conversation_id.as_deref())\n        else {\n            return CommandAcknowledgementV2::rejected(\n                Some(&command.envelope),\n                RejectionReason::StaleScope,\n            );\n        };\n        let chat_id = match crate::storage::ChatId::parse(conversation_id) {\n            Ok(value) => value,\n            Err(_) => {\n                return CommandAcknowledgementV2::rejected(\n                    Some(&command.envelope),\n                    RejectionReason::StaleScope,\n                )\n            }\n        };\n",
    "        let universal_parent = payload\n            .parent_node_id\n            .as_deref()\n            .and_then(parse_storage_node_id);\n        let chat_id = if universal_parent.is_none() {\n            let Some(conversation_id) = command\n                .envelope\n                .scope\n                .as_ref()\n                .and_then(|scope| scope.conversation_id.as_deref())\n            else {\n                return CommandAcknowledgementV2::rejected(\n                    Some(&command.envelope),\n                    RejectionReason::StaleScope,\n                );\n            };\n            match crate::storage::ChatId::parse(conversation_id) {\n                Ok(value) => Some(value),\n                Err(_) => {\n                    return CommandAcknowledgementV2::rejected(\n                        Some(&command.envelope),\n                        RejectionReason::StaleScope,\n                    )\n                }\n            }\n        } else {\n            None\n        };\n",
)
replace_once(
    "rust/crates/mukei-core/src/application_runtime/storage_import.rs",
    "            match importer\n                .import_workspace_file(\n                    crate::storage::WorkspaceStagedImportRequest {\n                        chat_id,\n                        staged_path: PathBuf::from(staged_path),\n                        original_filename: display_name,\n                        detected_mime: Some(mime_type),\n                        expected_size: Some(size_bytes),\n                        duplicate_policy: crate::storage::DuplicatePolicy::RenameNewEntry,\n                        source_uri_fingerprint: Some(source_fingerprint),\n                    },\n                    cancellation,\n                )\n                .await\n            {\n",
    "            let import_result = if let Some(parent_node_id) = universal_parent {\n                importer\n                    .import_universal_file(\n                        crate::storage::UniversalStagedImportRequest {\n                            parent_node_id,\n                            staged_path: PathBuf::from(staged_path),\n                            original_filename: display_name,\n                            detected_mime: Some(mime_type),\n                            expected_size: Some(size_bytes),\n                            duplicate_policy: crate::storage::DuplicatePolicy::RenameNewEntry,\n                            source_uri_fingerprint: Some(source_fingerprint),\n                        },\n                        cancellation,\n                    )\n                    .await\n            } else {\n                importer\n                    .import_workspace_file(\n                        crate::storage::WorkspaceStagedImportRequest {\n                            chat_id: chat_id.expect(\"workspace import validated chat id\"),\n                            staged_path: PathBuf::from(staged_path),\n                            original_filename: display_name,\n                            detected_mime: Some(mime_type),\n                            expected_size: Some(size_bytes),\n                            duplicate_policy: crate::storage::DuplicatePolicy::RenameNewEntry,\n                            source_uri_fingerprint: Some(source_fingerprint),\n                        },\n                        cancellation,\n                    )\n                    .await\n            };\n            match import_result {\n",
)

# Native composition: one SQLCipher-backed logical storage port beside importer.
replace_once(
    "rust/crates/mukei-android-jni/src/secure_runtime_jni.rs",
    "        Migrator, RuntimeProjectionRepository, StagedFileImporter, StagedPlaintextCleanup,\n        WorkspaceStagedImportService, DEFAULT_MAX_STAGED_IMPORT_BYTES,\n",
    "        Migrator, RuntimeProjectionRepository, SqlStorageWorkspaceService, StagedFileImporter,\n        StagedPlaintextCleanup, StorageWorkspacePort, WorkspaceStagedImportService,\n        DEFAULT_MAX_STAGED_IMPORT_BYTES,\n",
)
replace_once(
    "rust/crates/mukei-android-jni/src/secure_runtime_jni.rs",
    "            let mut services = crate::runtime_services(&config);\n            services.storage_importer = Some(importer);\n",
    "            let storage_workspace: Arc<dyn StorageWorkspacePort> =\n                Arc::new(SqlStorageWorkspaceService::new(Arc::clone(&database_pool)));\n            let mut services = crate::runtime_services(&config);\n            services.storage_importer = Some(importer);\n            services.storage_workspace = Some(storage_workspace);\n",
)

print("storage workspace M1 backend patch applied")
