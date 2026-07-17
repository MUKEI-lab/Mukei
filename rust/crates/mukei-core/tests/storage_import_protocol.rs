use chrono::Utc;
use mukei_core::ui_protocol::{
    validate_command, CommandEnvelopeV2, CommandScope, CommandType, ProtocolVersion,
    RejectionReason, ValidatedCommandPayload,
};
use serde_json::json;

fn command() -> CommandEnvelopeV2 {
    CommandEnvelopeV2 {
        protocol_version: ProtocolVersion::CURRENT,
        command_id: "command-storage-import".into(),
        request_id: "request-storage-import".into(),
        command_type: "storage.import_file".into(),
        submitted_at: Utc::now(),
        operation_id: None,
        correlation_id: "correlation-storage-import".into(),
        idempotency_key: Some("storage-import-chat-1-notes".into()),
        scope: Some(CommandScope {
            conversation_id: Some("chat-1".into()),
            branch_id: Some("branch-1".into()),
            turn_id: None,
            model_id: None,
            document_id: None,
        }),
        payload: json!({
            "target": "content://documents/notes",
            "display_name": "notes.txt",
            "mime_type": "text/plain"
        }),
    }
}

#[test]
fn scoped_content_uri_import_is_structurally_valid() {
    let validated = validate_command(command()).unwrap();
    assert_eq!(validated.command_type, CommandType::StorageImportFile);
    assert!(matches!(
        validated.payload,
        ValidatedCommandPayload::StorageImport(_)
    ));
}

#[test]
fn import_requires_conversation_scope() {
    let mut value = command();
    value.scope = None;
    assert_eq!(validate_command(value), Err(RejectionReason::StaleScope));
}

#[test]
fn import_rejects_non_content_targets() {
    let mut value = command();
    value.payload["target"] = json!("/sdcard/Download/notes.txt");
    assert_eq!(
        validate_command(value),
        Err(RejectionReason::InvalidPayload)
    );
}

#[test]
fn import_rejects_cross_domain_scope_fields() {
    let mut value = command();
    value.scope.as_mut().unwrap().model_id = Some("model-a".into());
    assert_eq!(validate_command(value), Err(RejectionReason::StaleScope));

    let mut value = command();
    value.scope.as_mut().unwrap().document_id = Some("document-a".into());
    assert_eq!(validate_command(value), Err(RejectionReason::StaleScope));
}
