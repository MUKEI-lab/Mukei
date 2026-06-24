//! TRD §13.3 — post-parse tool validator.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{MukeiError, Result};
use crate::types::ToolCallId;

#[derive(Debug, Clone, Deserialize)]
pub struct RawToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub struct TypedToolCall {
    pub id: ToolCallId,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    MismatchedArgs {
        tool: String,
        observed: Value,
    },
    UnknownTool(String),
    MissingRequiredField {
        tool: String,
        field: String,
    },
    WrongFieldType {
        tool: String,
        field: String,
        expected: &'static str,
        actual: String,
    },
    ConstraintViolation {
        tool: String,
        detail: String,
    },
}

const ALLOWED_FIELDS_PER_TOOL: &[(&str, &[&str])] = &[
    ("web_search", &["query"]),
    ("read_file", &["path"]),
    ("get_hardware_info", &[]),
    ("math_eval", &["expression"]),
];

pub fn parse_gbnf_output(response: &str) -> Result<Vec<RawToolCall>> {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<RawToolCall>>(trimmed)
        .map_err(|e| MukeiError::ToolParseFailed(e.to_string()))
}

pub fn validate_tool_calls(raw_calls: Vec<RawToolCall>) -> Result<Vec<TypedToolCall>> {
    let (accepted, errors) = validate(raw_calls);
    if errors.is_empty() {
        Ok(accepted)
    } else {
        Err(MukeiError::ToolArgsRejected {
            tool_name: "validator".to_string(),
            reason: format_for_llm(&errors),
        })
    }
}

pub fn validate(raw_calls: Vec<RawToolCall>) -> (Vec<TypedToolCall>, Vec<ValidationError>) {
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();

    for call in raw_calls {
        match validate_one(call) {
            Ok(call) => accepted.push(call),
            Err(error) => rejected.push(error),
        }
    }

    (accepted, rejected)
}

fn validate_one(call: RawToolCall) -> std::result::Result<TypedToolCall, ValidationError> {
    let allowed = match ALLOWED_FIELDS_PER_TOOL
        .iter()
        .find(|(name, _)| *name == call.name.as_str())
    {
        Some((_, fields)) => *fields,
        None => return Err(ValidationError::UnknownTool(call.name)),
    };

    let obj = call
        .arguments
        .as_object()
        .ok_or_else(|| ValidationError::WrongFieldType {
            tool: call.name.clone(),
            field: "arguments".to_string(),
            expected: "object",
            actual: call.arguments.to_string(),
        })?;

    let extras = obj
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !extras.is_empty() {
        return Err(ValidationError::MismatchedArgs {
            tool: call.name.clone(),
            observed: call.arguments,
        });
    }

    match call.name.as_str() {
        "web_search" => {
            let query = get_required_string(&call.name, &call.arguments, "query")?;
            if query.trim().is_empty() {
                return Err(ValidationError::ConstraintViolation {
                    tool: call.name,
                    detail: "query must be non-empty".to_string(),
                });
            }
            Ok(TypedToolCall {
                id: ToolCallId::default(),
                name: "web_search".to_string(),
                arguments: json!({ "query": query.trim() }),
            })
        }
        "read_file" => {
            let path = get_required_string(&call.name, &call.arguments, "path")?;
            if !path.starts_with("saf://") {
                return Err(ValidationError::ConstraintViolation {
                    tool: call.name,
                    detail: "path must begin with saf://".to_string(),
                });
            }
            if path.len() > 256 {
                return Err(ValidationError::ConstraintViolation {
                    tool: "read_file".to_string(),
                    detail: "path exceeds 256 bytes".to_string(),
                });
            }
            Ok(TypedToolCall {
                id: ToolCallId::default(),
                name: "read_file".to_string(),
                arguments: json!({ "path": path }),
            })
        }
        "get_hardware_info" => Ok(TypedToolCall {
            id: ToolCallId::default(),
            name: "get_hardware_info".to_string(),
            arguments: json!({}),
        }),
        "math_eval" => {
            let expression = get_required_string(&call.name, &call.arguments, "expression")?;
            if expression.trim().is_empty() {
                return Err(ValidationError::ConstraintViolation {
                    tool: call.name,
                    detail: "expression must be non-empty".to_string(),
                });
            }
            if expression.len() > 1024 {
                return Err(ValidationError::ConstraintViolation {
                    tool: "math_eval".to_string(),
                    detail: "expression exceeds 1024 bytes".to_string(),
                });
            }
            Ok(TypedToolCall {
                id: ToolCallId::default(),
                name: "math_eval".to_string(),
                arguments: json!({ "expression": expression }),
            })
        }
        other => Err(ValidationError::UnknownTool(other.to_string())),
    }
}

fn get_required_string<'a>(
    tool: &str,
    value: &'a Value,
    field: &str,
) -> std::result::Result<&'a str, ValidationError> {
    match value.get(field) {
        Some(v) => v.as_str().ok_or_else(|| ValidationError::WrongFieldType {
            tool: tool.to_string(),
            field: field.to_string(),
            expected: "string",
            actual: v.to_string(),
        }),
        None => Err(ValidationError::MissingRequiredField {
            tool: tool.to_string(),
            field: field.to_string(),
        }),
    }
}

pub fn format_for_llm(errors: &[ValidationError]) -> String {
    let mut out = String::from("Tool-call validation failed:\n");
    for error in errors {
        match error {
            ValidationError::UnknownTool(tool) => {
                out.push_str(&format!("- Unknown tool '{tool}'. Allowed: web_search, read_file, get_hardware_info, math_eval.\n"));
            }
            ValidationError::MismatchedArgs { tool, observed } => {
                out.push_str(&format!(
                    "- Tool '{tool}' had unexpected argument fields: {observed}.\n"
                ));
            }
            ValidationError::MissingRequiredField { tool, field } => {
                out.push_str(&format!(
                    "- Tool '{tool}' is missing required field '{field}'.\n"
                ));
            }
            ValidationError::WrongFieldType {
                tool,
                field,
                expected,
                actual,
            } => {
                out.push_str(&format!(
                    "- Tool '{tool}' field '{field}' must be {expected}; got {actual}.\n"
                ));
            }
            ValidationError::ConstraintViolation { tool, detail } => {
                out.push_str(&format!(
                    "- Tool '{tool}' violated a semantic constraint: {detail}.\n"
                ));
            }
        }
    }
    out.push_str("Re-emit ONLY valid JSON tool calls that match the documented schemas.");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cross_tool_arguments() {
        let raw = vec![RawToolCall {
            name: "web_search".to_string(),
            arguments: json!({"path": "saf://x"}),
        }];
        let (ok, err) = validate(raw);
        assert!(ok.is_empty());
        assert!(!err.is_empty());
    }

    #[test]
    fn rejects_raw_disk_paths() {
        let raw = vec![RawToolCall {
            name: "read_file".to_string(),
            arguments: json!({"path": "/sdcard/file.txt"}),
        }];
        let (_, err) = validate(raw);
        assert!(matches!(
            err[0],
            ValidationError::ConstraintViolation { .. }
        ));
    }

    #[test]
    fn read_file_rejects_every_non_saf_scheme() {
        // Architect review GH #38 (TRD §13.3): read_file MUST only
        // accept saf:// URIs. Any other scheme — file://, http://,
        // raw absolute, raw relative, even an empty string — is
        // rejected before dispatch.
        for bad in [
            "",
            "/etc/passwd",
            "./relative.txt",
            "file:///etc/passwd",
            "http://attacker.example/x",
            "https://attacker.example/x",
            "content://com.android.providers.media.documents/document/image%3A12345",
            "SAF://uppercase",
            "saf:/missing-second-slash",
        ] {
            let raw = vec![RawToolCall {
                name: "read_file".to_string(),
                arguments: json!({"path": bad}),
            }];
            let (ok, err) = validate(raw);
            assert!(
                ok.is_empty() && !err.is_empty(),
                "read_file accepted non-saf path `{bad}` — SAF enforcement broken",
            );
        }
    }

    #[test]
    fn accepts_valid_calls() {
        let raw = vec![
            RawToolCall {
                name: "web_search".to_string(),
                arguments: json!({"query": "rust ownership"}),
            },
            RawToolCall {
                name: "read_file".to_string(),
                arguments: json!({"path": "saf://deadbeef"}),
            },
            RawToolCall {
                name: "get_hardware_info".to_string(),
                arguments: json!({}),
            },
            RawToolCall {
                name: "math_eval".to_string(),
                arguments: json!({"expression": "2+2"}),
            },
        ];
        let (ok, err) = validate(raw);
        assert!(err.is_empty());
        assert_eq!(ok.len(), 4);
    }
}
