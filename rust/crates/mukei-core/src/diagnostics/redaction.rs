//! Privacy-safe redaction helpers for diagnostics.
//!
//! Production diagnostics must never include prompts, retrieved chunks,
//! document contents, file paths, API keys, database keys, or bearer
//! tokens. The helpers here intentionally prefer redacting too much over
//! preserving potentially sensitive context.

use std::path::Path;

const REDACTED_SECRET: &str = "[redacted-secret]";
const REDACTED_CONTENT: &str = "[redacted-content]";
const REDACTED_PATH: &str = "[redacted-path]";

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
        format!("{REDACTED_PATH}.{extension}")
    } else {
        REDACTED_PATH.to_string()
    }
}

pub fn sanitize_log_value(value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    if looks_like_secret(value) {
        REDACTED_SECRET.to_string()
    } else if looks_like_path(value) {
        REDACTED_PATH.to_string()
    } else {
        redact_inline_secrets(value)
    }
}

pub fn sanitize_error_message(message: impl AsRef<str>) -> String {
    sanitize_log_value(message)
}

fn looks_like_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("authorization:")
        || lower.contains("bearer ")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("access_token")
        || lower.contains("db_key")
        || lower.contains("database_cipher_key")
        || lower.contains("x-subscription-token")
        || value.starts_with("sk-")
        || value.starts_with("tvly-")
}

fn looks_like_path(value: &str) -> bool {
    value.starts_with("/data/")
        || value.starts_with("/sdcard/")
        || value.starts_with("/storage/")
        || value.starts_with("content://")
        || value.contains("\\Users\\")
        || value.contains("/Users/")
        || value.contains("/home/")
}

fn redact_inline_secrets(value: &str) -> String {
    value
        .split_whitespace()
        .map(|part| {
            if looks_like_secret(part) {
                REDACTED_SECRET
            } else if looks_like_path(part) {
                REDACTED_PATH
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
