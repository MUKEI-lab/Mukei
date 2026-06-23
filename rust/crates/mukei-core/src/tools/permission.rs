//! Tool permission matrix — bridge wiring scaffold (user priority #4).
//!
//! Per the v0.7.5 architecture amendment, every LLM-callable tool
//! declares an explicit set of [`Capability`] requirements (Internet,
//! Memory Read, Shell, Disk Read, …). The matrix is consulted BEFORE
//! the executor dispatches a tool call. A capability the user has not
//! enabled in `config.toml::[defaults]` (or that the OS has revoked,
//! e.g. via a SAF revoke) causes the dispatch to short-circuit into a
//! structured `tool_error` envelope with `FailureKind::Permanent`.
//!
//! This module is intentionally a **pure data scaffold** in this round.
//! The executor integration is gated on the bridge crate landing
//! `feature = "tool_permission_matrix"`. Until then, the matrix is
//! consulted only by the diagnostic / test paths so the data contract
//! is locked in without changing runtime behaviour.
//!
//! # Invariants
//!
//! - The matrix is **closed**: every tool name in
//!   [`crate::tools::ALLOWED_TOOLS`] MUST have a row in
//!   [`PermissionMatrix::default`]. The unit test
//!   `matrix_covers_every_allowed_tool` enforces this.
//! - Capabilities are **additive**: a tool that needs `Internet` AND
//!   `Disk_Read` declares both, and the user must have granted both.
//! - Denying a tool that's already been REGISTERED is a `Permanent`
//!   failure — the LLM is told the tool is permanently disabled and
//!   should not retry.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Capability buckets a tool may declare. Stable JSON tags — persisted
/// to `tool_audit_log.args_json` for forensics.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Outbound HTTP / DNS. Required by `web_search`.
    Internet,
    /// Read user files. Always SAF-scoped. Required by `read_file`.
    DiskRead,
    /// Write user files. Currently unused; reserved for future
    /// `write_file` / `save_session` tools.
    DiskWrite,
    /// Probe device state (CPU, thermal, RAM). Required by
    /// `get_hardware_info`.
    DeviceState,
    /// Read the conversation memory (chunks table, RAG vectors).
    /// Currently unused at the leaf-tool level — RAG retrieval is
    /// driven by the ReAct loop itself.
    MemoryRead,
    /// Execute a sandboxed evaluator (math, JSON-Schema, etc.). The
    /// `math_eval` tool gates on this rather than on `Internet`.
    SandboxEval,
    /// Spawn a subprocess or shell. **Never** granted in the v0.7.5
    /// architecture; included only so a future Mukei build that
    /// optionally enables a shell tool can flip a single bit.
    Shell,
}

impl Capability {
    /// Stable identifier used in audit-log JSON.
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Internet => "internet",
            Self::DiskRead => "disk_read",
            Self::DiskWrite => "disk_write",
            Self::DeviceState => "device_state",
            Self::MemoryRead => "memory_read",
            Self::SandboxEval => "sandbox_eval",
            Self::Shell => "shell",
        }
    }
}

/// Required capability set for one tool.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPermissions {
    /// Capabilities required to dispatch the tool. Empty set means the
    /// tool is unconditionally allowed (currently only used by future
    /// reserved tools).
    pub required: std::collections::BTreeSet<Capability>,
}

impl ToolPermissions {
    /// Construct a permission set from a slice. Order-independent.
    pub fn of(caps: &[Capability]) -> Self {
        Self {
            required: caps.iter().copied().collect(),
        }
    }
}

/// Matrix of tool-name \u2192 required capability set.
///
/// Use [`PermissionMatrix::is_allowed`] to check whether a tool can
/// dispatch given the user's currently-granted capability set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionMatrix {
    rows: BTreeMap<String, ToolPermissions>,
}

impl PermissionMatrix {
    /// Build an empty matrix — used by tests that want to register one
    /// tool at a time.
    pub fn empty() -> Self {
        Self { rows: BTreeMap::new() }
    }

    /// Add / replace one row.
    pub fn register(&mut self, tool: impl Into<String>, perms: ToolPermissions) {
        self.rows.insert(tool.into(), perms);
    }

    /// Permission set for `tool`, or `None` if the tool is not in the
    /// matrix. A `None` is a **stronger** denial than an empty set: it
    /// signals the tool name itself is unknown and the LLM should not
    /// keep calling it.
    pub fn permissions_for(&self, tool: &str) -> Option<&ToolPermissions> {
        self.rows.get(tool)
    }

    /// True iff every required capability for `tool` appears in the
    /// `granted` set. Unknown tools return `false`.
    pub fn is_allowed(
        &self,
        tool: &str,
        granted: &std::collections::BTreeSet<Capability>,
    ) -> bool {
        let Some(perms) = self.permissions_for(tool) else {
            return false;
        };
        perms.required.iter().all(|c| granted.contains(c))
    }

    /// Iterate every `(tool, perms)` pair. The order is `BTreeMap`-stable
    /// so audit-log JSON is deterministic.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ToolPermissions)> {
        self.rows.iter().map(|(k, v)| (k.as_str(), v))
    }
}

impl Default for PermissionMatrix {
    /// Canonical v0.7.5 matrix — must cover every tool in
    /// [`crate::tools::ALLOWED_TOOLS`].
    fn default() -> Self {
        let mut m = Self::empty();
        m.register(
            "web_search",
            ToolPermissions::of(&[Capability::Internet]),
        );
        m.register(
            "read_file",
            ToolPermissions::of(&[Capability::DiskRead]),
        );
        m.register(
            "get_hardware_info",
            ToolPermissions::of(&[Capability::DeviceState]),
        );
        m.register(
            "math_eval",
            ToolPermissions::of(&[Capability::SandboxEval]),
        );
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn matrix_covers_every_allowed_tool() {
        // Invariant: every name in ALLOWED_TOOLS has a row in the
        // default matrix. Adding a new tool requires updating BOTH.
        let m = PermissionMatrix::default();
        for tool in crate::tools::ALLOWED_TOOLS {
            assert!(
                m.permissions_for(tool).is_some(),
                "tool '{tool}' is in ALLOWED_TOOLS but has no permission matrix row",
            );
        }
    }

    #[test]
    fn is_allowed_requires_full_capability_set() {
        let m = PermissionMatrix::default();
        let mut granted = BTreeSet::new();
        // No capabilities — nothing dispatches.
        assert!(!m.is_allowed("web_search", &granted));
        granted.insert(Capability::Internet);
        assert!(m.is_allowed("web_search", &granted));
        assert!(!m.is_allowed("read_file", &granted));
        granted.insert(Capability::DiskRead);
        assert!(m.is_allowed("read_file", &granted));
    }

    #[test]
    fn unknown_tool_is_never_allowed_even_with_full_capabilities() {
        let m = PermissionMatrix::default();
        let granted: BTreeSet<Capability> = [
            Capability::Internet,
            Capability::DiskRead,
            Capability::DeviceState,
            Capability::SandboxEval,
            Capability::Shell,
        ]
        .into_iter()
        .collect();
        assert!(!m.is_allowed("never_existed", &granted));
    }

    #[test]
    fn capability_tags_are_stable_snake_case() {
        for c in [
            Capability::Internet,
            Capability::DiskRead,
            Capability::DiskWrite,
            Capability::DeviceState,
            Capability::MemoryRead,
            Capability::SandboxEval,
            Capability::Shell,
        ] {
            let t = c.as_tag();
            assert!(t.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'));
        }
    }

    #[test]
    fn shell_capability_is_declared_but_unused_by_default_matrix() {
        // Defence in depth: no tool in the default matrix may require
        // the `Shell` capability. If a future tool ever does, it must
        // be added to ALLOWED_TOOLS AND TRD \u00a75 updated to document the
        // sandbox boundary.
        let m = PermissionMatrix::default();
        for (tool, perms) in m.iter() {
            assert!(
                !perms.required.contains(&Capability::Shell),
                "tool '{tool}' declares Capability::Shell — this is reserved and must not be required by default",
            );
        }
    }
}
