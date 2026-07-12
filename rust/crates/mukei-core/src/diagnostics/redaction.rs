//! Privacy-safe redaction helpers for diagnostics.
//!
//! Production diagnostics must never include prompts, retrieved chunks,
//! document contents, file paths, API keys, database keys, bearer tokens,
//! or identifying URLs. The helpers here intentionally prefer redacting
//! too much over preserving potentially sensitive context.
//!
//! The existing public helpers remain the compatibility surface. New
//! structured diagnostics use [`sanitize_telemetry_text`] as the canonical
//! path before data can reach an event buffer or sink.

use std::path::Path;

const REDACTED_SECRET: &str = "[redacted-secret]";
const REDACTED_CONTENT: &str = "[redacted-content]";
const REDACTED_PATH: &str = "[redacted-path]";
const REDACTED_IDENTIFIER: &str = "[redacted-identifier]";
const REDACTED_CONTENT_URI: &str = "[redacted-content-uri]";
const REDACTED_URL: &str = "[redacted-url]";
const MAX_DIAGNOSTIC_TEXT_CHARS: usize = 4_096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SanitizedText {
    value: String,
}

impl SanitizedText {
    pub(crate) fn into_string(self) -> String {
        self.value
    }
}

pub fn redact_secret<T: ?Sized>(_value: &T) -> &'static str {
    REDACTED_SECRET
}

pub fn redact_content<T: ?Sized>(_value: &T) -> &'static str {
    REDACTED_CONTENT
}

pub fn redact_path(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    if path.as_os_str().is_empty() {
        REDACTED_PATH.to_string()
    } else if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        let extension = sanitize_extension(extension);
        if extension.is_empty() {
            REDACTED_PATH.to_string()
        } else {
            format!("{REDACTED_PATH}.{extension}")
        }
    } else {
        REDACTED_PATH.to_string()
    }
}

/// Compatibility helper for existing logging callers.
///
/// This now delegates to the same canonical sanitizer used by structured
/// diagnostics. No caller needs to change its API usage.
pub fn sanitize_log_value(value: impl AsRef<str>) -> String {
    sanitize_telemetry_text(value.as_ref(), MAX_DIAGNOSTIC_TEXT_CHARS).into_string()
}

pub fn sanitize_error_message(message: impl AsRef<str>) -> String {
    sanitize_log_value(message)
}

/// Canonical sanitizer for all newly structured diagnostic text.
///
/// It removes control characters, redacts secret shapes, paths and content
/// URIs, coarsens URLs, and bounds the resulting string by Unicode scalar
/// count. It never returns the raw input for recognized sensitive shapes.
pub(crate) fn sanitize_telemetry_text(value: &str, max_chars: usize) -> SanitizedText {
    // The sanitizer itself is memory-bounded. Callers may hand us an already
    // allocated large string, but diagnostics never duplicate the full input
    // merely to sanitize a short telemetry field or log line.
    let effective_max = max_chars.min(MAX_DIAGNOSTIC_TEXT_CHARS);
    let scan_limit = effective_max
        .saturating_mul(4)
        .max(64)
        .min(MAX_DIAGNOSTIC_TEXT_CHARS);
    let (normalized, _) = normalize_controls_bounded(value, scan_limit);
    let trimmed = normalized.trim();

    if trimmed.is_empty() {
        return SanitizedText {
            value: String::new(),
        };
    }

    if looks_like_secret(trimmed) {
        return SanitizedText {
            value: REDACTED_SECRET.to_string(),
        };
    }

    let sanitized = trimmed
        .split_whitespace()
        .map(|part| sanitize_token(part).0)
        .collect::<Vec<_>>()
        .join(" ");

    let (bounded, _) = truncate_chars(&sanitized, effective_max);
    SanitizedText { value: bounded }
}

/// Sanitize a structured telemetry field using both its allowlisted key and
/// the value shape. High-risk field names are redacted even when a caller
/// accidentally marks them as operational-safe. This is deliberately
/// conservative: observability should carry categories and identifiers, not
/// user content.
pub(crate) fn sanitize_telemetry_field(
    key: &str,
    value: &str,
    max_chars: usize,
) -> SanitizedText {
    if let Some(redacted) = telemetry_field_redaction(key) {
        return SanitizedText {
            value: redacted.to_string(),
        };
    }

    sanitize_telemetry_text(value, max_chars)
}

/// Return a fixed redaction marker for field names that must never retain the
/// caller-supplied value, regardless of whether the value was passed as text,
/// a supposedly stable identifier, or a scalar.
pub(crate) fn telemetry_field_redaction(key: &str) -> Option<&'static str> {
    let lower = key.to_ascii_lowercase();

    if matches_field_key(
        &lower,
        &[
            "authorization",
            "authorization_header",
            "api_key",
            "apikey",
            "access_token",
            "refresh_token",
            "client_secret",
            "password",
            "db_key",
            "database_key",
            "wrapped_key",
            "key_blob",
            "secret",
            "token",
        ],
        &[
            "_authorization",
            "_authorization_header",
            "_api_key",
            "_access_token",
            "_refresh_token",
            "_client_secret",
            "_password",
            "_db_key",
            "_database_key",
            "_wrapped_key",
            "_key_blob",
            "_secret",
        ],
    ) {
        return Some(REDACTED_SECRET);
    }

    if matches_field_key(
        &lower,
        &[
            "prompt",
            "completion",
            "assistant_content",
            "assistant_message",
            "user_content",
            "user_message",
            "raw_content",
            "document_content",
            "document_text",
            "chunk_text",
            "query_text",
            "input_text",
            "output_text",
            "message_body",
        ],
        &[
            "_prompt",
            "_completion",
            "_content",
            "_message",
            "_text",
            "_body",
        ],
    ) {
        return Some(REDACTED_CONTENT);
    }

    if matches_field_key(
        &lower,
        &[
            "file_path",
            "filepath",
            "private_path",
            "document_path",
            "directory_path",
            "document_name",
            "filename",
            "file_name",
        ],
        &[
            "_file_path",
            "_filepath",
            "_private_path",
            "_document_path",
            "_directory_path",
            "_document_name",
            "_filename",
            "_file_name",
        ],
    ) {
        return Some(REDACTED_PATH);
    }

    if matches_field_key(
        &lower,
        &[
            "device_id",
            "device_identifier",
            "installation_id",
            "advertising_id",
            "hardware_id",
            "serial_number",
            "customer_id",
            "tenant_id",
        ],
        &[
            "_device_id",
            "_device_identifier",
            "_installation_id",
            "_advertising_id",
            "_hardware_id",
            "_serial_number",
            "_customer_id",
            "_tenant_id",
        ],
    ) {
        return Some(REDACTED_IDENTIFIER);
    }

    None
}

fn matches_field_key(value: &str, exact: &[&str], suffixes: &[&str]) -> bool {
    exact.iter().any(|candidate| value == *candidate)
        || suffixes.iter().any(|suffix| value.ends_with(suffix))
}

/// Validate a stable machine identifier such as an event, component,
/// attribute, metric or reason-code name.
///
/// Only a deliberately small ASCII alphabet is accepted. This prevents
/// arbitrary user text, paths and URLs from becoming cardinality-bearing
/// identities. The value is rejected rather than silently normalized.
pub(crate) fn sanitize_stable_identifier(value: &str, max_len: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_len || looks_like_secret(value) {
        return None;
    }

    if value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'))
    {
        Some(value.to_string())
    } else {
        None
    }
}

/// Returns false for values that are unsafe even as opaque correlation input.
pub(crate) fn is_safe_opaque_input(value: &str, max_len: usize) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= max_len
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'@')
        })
        && !looks_like_secret(value)
        && !looks_like_path(value)
        && !looks_like_url(value)
        && !value.starts_with("content://")
}

fn sanitize_token(part: &str) -> (String, bool) {
    let stripped = part.trim_matches(|c: char| {
        matches!(c, ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'')
    });

    if looks_like_secret(stripped) {
        return (REDACTED_SECRET.to_string(), true);
    }
    if stripped.starts_with("content://") {
        return (REDACTED_CONTENT_URI.to_string(), true);
    }
    if looks_like_path(stripped) {
        return (REDACTED_PATH.to_string(), true);
    }
    if looks_like_url(stripped) {
        return (coarse_url_class(stripped), true);
    }

    (part.to_string(), false)
}

fn looks_like_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("authorization:")
        || lower.contains("bearer ")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("access_token")
        || lower.contains("refresh_token")
        || lower.contains("client_secret")
        || lower.contains("db_key")
        || lower.contains("database_cipher_key")
        || lower.contains("x-subscription-token")
        || lower.contains("password=")
        || lower.contains("token=")
        || value.starts_with("sk-")
        || value.starts_with("tvly-")
}

fn looks_like_path(value: &str) -> bool {
    value.starts_with("/data/")
        || value.starts_with("/sdcard/")
        || value.starts_with("/storage/")
        || value.starts_with("/home/")
        || value.starts_with("/Users/")
        || value.starts_with("file://")
        || value.starts_with("content://")
        || value.contains("\\Users\\")
        || (value.len() > 3
            && value.as_bytes()[1] == b':'
            && matches!(value.as_bytes()[2], b'\\' | b'/'))
}

fn looks_like_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ws://")
        || lower.starts_with("wss://")
}

fn coarse_url_class(value: &str) -> String {
    let scheme = value
        .split_once("://")
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .filter(|scheme| matches!(scheme.as_str(), "http" | "https" | "ws" | "wss"));

    match scheme {
        Some(scheme) => format!("{REDACTED_URL}:{scheme}"),
        None => REDACTED_URL.to_string(),
    }
}

fn normalize_controls_bounded(value: &str, max_chars: usize) -> (String, bool) {
    let mut normalized = String::with_capacity(max_chars.min(value.len()));
    let mut chars = value.chars();
    for _ in 0..max_chars {
        let Some(ch) = chars.next() else {
            return (normalized, false);
        };
        normalized.push(if ch.is_control() { ' ' } else { ch });
    }
    (normalized, chars.next().is_some())
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    if max_chars == usize::MAX || value.chars().count() <= max_chars {
        return (value.to_string(), false);
    }
    if max_chars == 0 {
        return (String::new(), true);
    }
    if max_chars == 1 {
        return ("…".to_string(), true);
    }

    let mut output = value.chars().take(max_chars - 1).collect::<String>();
    output.push('…');
    (output, true)
}

fn sanitize_extension(extension: &str) -> String {
    extension
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(12)
        .collect::<String>()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_secret_shapes() {
        assert_eq!(sanitize_log_value("sk-secret"), REDACTED_SECRET);
        assert_eq!(
            sanitize_log_value("Authorization: Bearer abc123"),
            REDACTED_SECRET
        );
        assert_eq!(
            sanitize_log_value("x-subscription-token=abc123"),
            REDACTED_SECRET
        );
    }


    #[test]
    fn sanitizer_does_not_preserve_unbounded_input() {
        let raw = "x".repeat(MAX_DIAGNOSTIC_TEXT_CHARS * 8);
        let safe = sanitize_log_value(raw);
        assert!(safe.chars().count() <= MAX_DIAGNOSTIC_TEXT_CHARS);
    }

    #[test]
    fn redacts_mobile_paths() {
        assert_eq!(
            sanitize_log_value("/sdcard/Documents/private.txt"),
            REDACTED_PATH
        );
        assert_eq!(
            sanitize_log_value("failed to open /data/user/0/app/key.db"),
            format!("failed to open {REDACTED_PATH}")
        );
    }

    #[test]
    fn path_redaction_preserves_only_extension() {
        assert_eq!(
            redact_path("/sdcard/private/model.gguf"),
            "[redacted-path].gguf"
        );
        assert_eq!(redact_path("/sdcard/private"), REDACTED_PATH);
    }
}
