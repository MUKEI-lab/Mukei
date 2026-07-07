//! Shared types — every conversation / tool / agent domain type lives here
//! so the bridge crate, the diagnostic crate, and the unit tests share
//! exactly the same shape. **No type may live outside `types` if it ever
//! crosses the FFI** — that is a hard rule because drift across the
//! boundary is the single largest defect class for CXX-Qt apps.
//!
//! TRD §1.2.3 / §2.3 / §2.5 / AF §8-9.

#![allow(clippy::derive_partial_eq_without_eq)] // UUIDs are fine without eq

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------
/// Strongly-typed conversation identifier. SQL primary key.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConversationId(pub Uuid);

/// Strongly-typed message identifier.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

/// Strongly-typed branch identifier (PRD §27 REQ-CHAT-02 / REQ-CHAT-06).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BranchId(pub Uuid);

/// Strongly-typed tool-call identifier (BS §2.6 tool_audit_log).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolCallId(pub Uuid);

// ---------------------------------------------------------------------
// Architect review GH #2 (tracker #1) — explicit constructors.
//
// `Default::default()` on each id type DOES return `Uuid::new_v4()`
// today, so callers that use `::default()` are correct. But that
// behaviour is non-obvious — `Default` *typically* means "the zero
// value". The architect review flagged this as a maintenance trap:
// a future refactor that swaps to `Default::derive` would silently
// downgrade every id to `Uuid::nil()` and collapse the branch DAG.
//
// We add explicit `::new()` constructors with a load-bearing docstring
// so the agent loop and every other call site can switch to a name
// that says exactly what it does. The `Default` impls below remain
// for backwards compatibility but now forward to `::new()` so the two
// can never drift.
// ---------------------------------------------------------------------

impl ConversationId {
    /// Mint a fresh random v4 conversation id.
    #[inline]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl MessageId {
    /// Mint a fresh random v4 message id.
    ///
    /// **Prefer this over `Default::default()` at every call site.**
    /// The branch DAG (BS §2 / V004) keys on this id as the parent
    /// pointer; two messages sharing an id would corrupt the tree.
    #[inline]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl BranchId {
    /// Mint a fresh random v4 branch id.
    #[inline]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl ToolCallId {
    /// Mint a fresh random v4 tool-call id.
    #[inline]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ConversationId {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for BranchId {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for ToolCallId {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------
// Chat model
// ---------------------------------------------------------------------
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
    /// RED_TEAM sentinel for FMEA failsafe tests.
    RedTeam,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: MessageId,
    pub role: Role,
    /// Branch this message belongs to (BS §2 — REQ-CHAT-06 branching).
    pub branch: BranchId,
    /// True if the message is part of the conversation main timeline.
    pub is_active: bool,
    /// RFC3339 creation timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Markdown source (assistant / user) or raw tool output (tool).
    pub content: String,
    /// Optional parent message — enables branching / fan-out tree.
    pub parent: Option<MessageId>,
    /// Optional token count, set when the assistant message is finalised.
    pub token_count: Option<u32>,
}

impl ChatMessage {
    pub fn user(branch: BranchId, content: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            role: Role::User,
            branch,
            is_active: true,
            created_at: chrono::Utc::now(),
            content: content.into(),
            parent: None,
            token_count: None,
        }
    }
}

// ---------------------------------------------------------------------
// Tool calling
// ---------------------------------------------------------------------
/// A language-model emitted tool call (post-`GBNF` parse, pre-validator).
/// The validator in `crate::tools::validator` (TRD §13.3) downgrades to a
/// typed `TypedToolCall` after stripping unknown fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub call_id: String, // raw LLM-emitted id
    pub name: String,
    pub arguments: serde_json::Value,
    /// When the validator rejected the call, this points to the marker
    /// the agent loop appends back into the LLM context (§2.3).
    pub rejected_reason: Option<String>,
}

/// Result of a `ToolCall` execution. Buffered into history as a
/// `Role::Tool` message wrapped in `<external_data trust="…">` (§5.2).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: ToolCallId,
    pub name: String,
    /// Body serialised as string (markdown / JSON / plaintext depending
    /// on the tool). Wrapped in prompt-injection sentinels before
    /// insertion into LLM context.
    pub output: String,
    /// True if the tool returned successfully. False routes the call
    /// through the graceful-degrade path: the executor classifies the
    /// failure via [`crate::agent::tools::FailureKind`], counts it
    /// against [`crate::agent::tools::ToolExecutionPolicy::max_failures_per_tool`],
    /// renders a structured `<external_data source="tool_error">`
    /// envelope, and lets the LLM produce a recovery answer. The agent
    /// loop is NEVER hard-aborted on `ok == false` (Issue #10 / #20 —
    /// the old hard-abort design is gone).
    pub ok: bool,
    /// Wall-clock duration of execution.
    pub took: std::time::Duration,
    /// Trust label — used by XML sandboxing (§12.2).
    pub trust: String, // "computed" | "untrusted" | "trusted"
}

// ---------------------------------------------------------------------
// Pre-typed AST for markdown rendering (TRD §35.1.1 — STRICT: no QML regex)
// ---------------------------------------------------------------------
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MarkdownNode {
    Paragraph {
        children: Vec<InlineNode>,
    },
    Heading {
        level: u8,
        children: Vec<InlineNode>,
    },
    CodeFence {
        language: String,
        content: String,
    },
    BulletList {
        items: Vec<Vec<InlineNode>>,
    },
    OrderedList {
        items: Vec<Vec<InlineNode>>,
    },
    Quote {
        children: Vec<InlineNode>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InlineNode {
    Text {
        value: String,
    },
    Bold {
        children: Vec<InlineNode>,
    },
    Italic {
        children: Vec<InlineNode>,
    },
    Code {
        value: String,
    },
    Link {
        text: String,
        href: String,
    },
    /// Used for `<external_data>` sentinel blocks (TRD §12.2).
    Sentinel {
        rule: String,
        body: String,
        trust: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_message_serialises_with_role_lowercase() {
        let json = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(json, "\"assistant\"");
    }

    #[test]
    fn markdown_node_roundtrips() {
        let n = MarkdownNode::Paragraph {
            children: vec![
                InlineNode::Text {
                    value: "hello ".into(),
                },
                InlineNode::Bold {
                    children: vec![InlineNode::Text {
                        value: "world".into(),
                    }],
                },
            ],
        };
        let s = serde_json::to_string(&n).unwrap();
        let de: MarkdownNode = serde_json::from_str(&s).unwrap();
        assert!(matches!(de, MarkdownNode::Paragraph { .. }));
    }
}
