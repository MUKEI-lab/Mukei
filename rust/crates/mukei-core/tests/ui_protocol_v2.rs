#[path = "../src/ui_protocol.rs"]
mod ui_protocol;

use chrono::Utc;
use serde_json::json;
use ui_protocol::*;

fn scope() -> CommandScope {
    CommandScope {
        conversation_id: Some("conversation-a".into()),
        branch_id: Some("branch-a".into()),
        turn_id: Some("turn-a".into()),
        model_id: None,
        document_id: None,
    }
}

fn send_command() -> CommandEnvelopeV2 {
    CommandEnvelopeV2 {
        protocol_version: ProtocolVersion::CURRENT,
        command_id: "command-a".into(),
        request_id: "request-a".into(),
        command_type: "chat.send_message".into(),
        submitted_at: Utc::now(),
        operation_id: None,
        correlation_id: "correlation-a".into(),
        idempotency_key: Some("idem-a".into()),
        scope: Some(scope()),
        payload: json!({"text": "hello"}),
    }
}

#[test]
fn accepted_ack_requires_authoritative_operation_id() {
    let command = send_command();
    let mut ack = CommandAcknowledgementV2::accepted(&command, None);
    assert!(!ack.validate_for(&command));
    ack.operation_id = Some("operation-a".into());
    assert!(ack.validate_for(&command));
}

#[test]
fn acknowledgement_must_match_transport_correlation() {
    let command = send_command();
    let mut ack =
        CommandAcknowledgementV2::accepted(&command, Some("operation-a".into()));
    ack.correlation_id = "different".into();
    assert!(!ack.validate_for(&command));
}

#[test]
fn scoped_chat_cancel_requires_target_operation_id() {
    let mut cancel = CommandEnvelopeV2 {
        protocol_version: ProtocolVersion::CURRENT,
        command_id: "command-cancel".into(),
        request_id: "request-cancel".into(),
        command_type: "chat.stop_generation".into(),
        submitted_at: Utc::now(),
        operation_id: None,
        correlation_id: "correlation-cancel".into(),
        idempotency_key: Some("cancel-operation-a".into()),
        scope: Some(scope()),
        payload: json!({}),
    };
    assert_eq!(
        validate_command(cancel.clone()),
        Err(RejectionReason::StaleScope)
    );
    cancel.operation_id = Some("operation-a".into());
    assert!(validate_command(cancel).is_ok());
}

#[test]
fn scoped_chat_cancel_requires_conversation_and_branch() {
    let mut cancel = CommandEnvelopeV2 {
        protocol_version: ProtocolVersion::CURRENT,
        command_id: "command-cancel".into(),
        request_id: "request-cancel".into(),
        command_type: "chat.stop_generation".into(),
        submitted_at: Utc::now(),
        operation_id: Some("operation-a".into()),
        correlation_id: "correlation-cancel".into(),
        idempotency_key: Some("cancel-operation-a".into()),
        scope: Some(scope()),
        payload: json!({}),
    };
    cancel.scope.as_mut().unwrap().branch_id = None;
    assert_eq!(validate_command(cancel), Err(RejectionReason::StaleScope));
}

#[test]
fn protocol_capabilities_truthfully_include_scoped_chat_operations() {
    let snapshot = ProtocolCapabilitySnapshot::current();
    assert!(snapshot
        .capabilities
        .iter()
        .any(|value| value == CAP_SCOPED_CHAT_OPERATIONS));
}

#[test]
fn unsupported_protocol_major_fails_closed() {
    let mut command = send_command();
    command.protocol_version.major = PROTOCOL_MAJOR + 1;
    assert_eq!(
        validate_command(command),
        Err(RejectionReason::UnsupportedProtocol)
    );
}
