from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if new in text:
        return text
    if old not in text:
        raise SystemExit(f"missing patch marker: {label}")
    return text.replace(old, new, 1)


runtime_path = Path("rust/crates/mukei-core/src/application_runtime.rs")
runtime = runtime_path.read_text(encoding="utf-8")
runtime = replace_once(
    runtime,
    'include!("application_runtime/documents_snapshot.rs");\ninclude!("application_runtime/tests.rs");',
    'include!("application_runtime/documents_snapshot.rs");\ninclude!("application_runtime/storage_import.rs");\ninclude!("application_runtime/tests.rs");\ninclude!("application_runtime/storage_import_tests.rs");',
    "application runtime includes",
)
runtime_path.write_text(runtime, encoding="utf-8")

types_path = Path("rust/crates/mukei-core/src/application_runtime/foundation_types.rs")
types = types_path.read_text(encoding="utf-8")
types = replace_once(
    types,
    "    pub backend_factory: Option<Arc<dyn InferenceBackendFactory>>,\n}",
    '    pub backend_factory: Option<Arc<dyn InferenceBackendFactory>>,\n    /// Encrypted staged-file importer. Absence keeps storage import capability hidden.\n    #[cfg(feature = "rusqlite")]\n    pub storage_importer: Option<Arc<dyn crate::storage::StagedFileImporter>>,\n}',
    "RuntimeServices storage importer",
)
types_path.write_text(types, encoding="utf-8")

struct_path = Path("rust/crates/mukei-core/src/application_runtime/foundation_runtime_struct.rs")
struct_text = struct_path.read_text(encoding="utf-8")
struct_text = replace_once(
    struct_text,
    "    rag_service: RwLock<Option<Arc<dyn RuntimeRagService>>>,\n    remote_tool_secrets:",
    '    rag_service: RwLock<Option<Arc<dyn RuntimeRagService>>>,\n    #[cfg(feature = "rusqlite")]\n    storage_importer: RwLock<Option<Arc<dyn crate::storage::StagedFileImporter>>>,\n    remote_tool_secrets:',
    "MukeiRuntime storage importer field",
)
struct_path.write_text(struct_text, encoding="utf-8")

base_path = Path("rust/crates/mukei-core/src/application_runtime/base.rs")
base = base_path.read_text(encoding="utf-8")
base = replace_once(
    base,
    "            rag_service: RwLock::new(None),\n            remote_tool_secrets:",
    '            rag_service: RwLock::new(None),\n            #[cfg(feature = "rusqlite")]\n            storage_importer: RwLock::new(services.storage_importer),\n            remote_tool_secrets:',
    "runtime constructor storage importer",
)
base = replace_once(
    base,
    '        #[cfg(feature = "network")]\n        commands.push(CommandType::ModelDownload);\n        ProtocolCapabilitySnapshot::for_commands(&commands)',
    '        #[cfg(feature = "network")]\n        commands.push(CommandType::ModelDownload);\n        #[cfg(feature = "rusqlite")]\n        if self\n            .storage_importer\n            .read()\n            .unwrap_or_else(|poisoned| poisoned.into_inner())\n            .is_some()\n        {\n            commands.push(CommandType::StorageImportFile);\n        }\n        ProtocolCapabilitySnapshot::for_commands(&commands)',
    "truthful storage import capability",
)
base_path.write_text(base, encoding="utf-8")

context_path = Path("rust/crates/mukei-core/src/application_runtime/foundation_context.rs")
context = context_path.read_text(encoding="utf-8")
context = replace_once(
    context,
    "            CommandType::DocumentRetryIngestion => runtime.retry_document_ingestion(command),\n            CommandType::SettingsUpdate =>",
    "            CommandType::DocumentRetryIngestion => runtime.retry_document_ingestion(command),\n            CommandType::StorageImportFile => runtime.import_storage_file(command),\n            CommandType::SettingsUpdate =>",
    "command router storage import arm",
)
context_path.write_text(context, encoding="utf-8")

services_path = Path("rust/crates/mukei-core/src/application_runtime/services.rs")
services = services_path.read_text(encoding="utf-8")
services = replace_once(
    services,
    "    /// Install transient, already-unwrapped provider credentials.\n    pub fn configure_remote_tools(",
    '    /// Attach the encrypted staged-file importer before capability negotiation.\n    #[cfg(feature = "rusqlite")]\n    pub fn attach_storage_importer(\n        &self,\n        importer: Arc<dyn crate::storage::StagedFileImporter>,\n    ) {\n        *self\n            .storage_importer\n            .write()\n            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(importer);\n    }\n\n    /// Install transient, already-unwrapped provider credentials.\n    pub fn configure_remote_tools(',
    "attach storage importer service",
)
services_path.write_text(services, encoding="utf-8")

protocol_path = Path("rust/crates/mukei-core/src/ui_protocol.rs")
protocol = protocol_path.read_text(encoding="utf-8")
protocol = protocol.replace(
    "pub const PROTOCOL_MINOR: u16 = 0;",
    "pub const PROTOCOL_MINOR: u16 = 1;",
    1,
)
protocol = replace_once(
    protocol,
    "    /// Retry document ingestion.\n    DocumentRetryIngestion,\n    /// Persist one product setting.",
    "    /// Retry document ingestion.\n    DocumentRetryIngestion,\n    /// Import a selected Android document into the active chat workspace.\n    StorageImportFile,\n    /// Persist one product setting.",
    "CommandType storage variant",
)
protocol = replace_once(
    protocol,
    '            "document.retry_ingestion" => Some(Self::DocumentRetryIngestion),\n            "settings.update" =>',
    '            "document.retry_ingestion" => Some(Self::DocumentRetryIngestion),\n            "storage.import_file" => Some(Self::StorageImportFile),\n            "settings.update" =>',
    "CommandType parse storage import",
)
protocol = replace_once(
    protocol,
    '            Self::DocumentRetryIngestion => "document.retry_ingestion",\n            Self::SettingsUpdate =>',
    '            Self::DocumentRetryIngestion => "document.retry_ingestion",\n            Self::StorageImportFile => "storage.import_file",\n            Self::SettingsUpdate =>',
    "CommandType string storage import",
)
protocol = replace_once(
    protocol,
    "                | Self::DocumentRetryIngestion\n                | Self::RecoveryResume",
    "                | Self::DocumentRetryIngestion\n                | Self::StorageImportFile\n                | Self::RecoveryResume",
    "storage import idempotency",
)
protocol = replace_once(
    protocol,
    "/// Payload containing one document identity.\n#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\npub struct DocumentPayload {",
    '/// Android document selected for encrypted workspace import.\n#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\npub struct StorageImportPayload {\n    /// Opaque `content://` URI handled only by the Android document port.\n    pub target: String,\n    /// User-visible filename validated again by storage admission policy.\n    pub display_name: String,\n    /// MIME type reported by Android.\n    pub mime_type: String,\n}\n\n/// Payload containing one document identity.\n#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\npub struct DocumentPayload {',
    "StorageImportPayload type",
)
protocol = replace_once(
    protocol,
    "    /// Document identity.\n    Document(DocumentPayload),\n    /// Setting mutation.",
    "    /// Document identity.\n    Document(DocumentPayload),\n    /// Selected document destined for encrypted workspace storage.\n    StorageImport(StorageImportPayload),\n    /// Setting mutation.",
    "validated storage payload variant",
)
protocol = replace_once(
    protocol,
    "            CommandType::RecoveryResume | CommandType::RecoveryRegenerate\n        ) {",
    "            CommandType::RecoveryResume\n                | CommandType::RecoveryRegenerate\n                | CommandType::StorageImportFile\n        ) {",
    "storage import requires scope",
)
protocol = replace_once(
    protocol,
    "        (\n            CommandType::DocumentRevoke | CommandType::DocumentRetryIngestion,\n            ValidatedCommandPayload::Document(value),\n        ) => {",
    "        (CommandType::StorageImportFile, ValidatedCommandPayload::StorageImport(_)) => {\n            if scope.conversation_id.is_none() || has_model || has_document {\n                return Err(RejectionReason::StaleScope);\n            }\n        }\n        (\n            CommandType::DocumentRevoke | CommandType::DocumentRetryIngestion,\n            ValidatedCommandPayload::Document(value),\n        ) => {",
    "storage import scope validation",
)
protocol = replace_once(
    protocol,
    "        CommandType::DocumentRevoke | CommandType::DocumentRetryIngestion => {\n            let value: DocumentPayload",
    '        CommandType::StorageImportFile => {\n            let value: StorageImportPayload = serde_json::from_value(envelope.payload.clone())\n                .map_err(|_| RejectionReason::InvalidPayload)?;\n            if !non_empty_bounded(&value.target, 8192)\n                || !value.target.starts_with("content://")\n                || value.target.chars().any(char::is_control)\n                || !non_empty_bounded(&value.display_name, 512)\n                || !non_empty_bounded(&value.mime_type, 256)\n            {\n                return Err(RejectionReason::InvalidPayload);\n            }\n            ValidatedCommandPayload::StorageImport(value)\n        }\n        CommandType::DocumentRevoke | CommandType::DocumentRetryIngestion => {\n            let value: DocumentPayload',
    "storage import payload validation",
)
protocol_path.write_text(protocol, encoding="utf-8")
