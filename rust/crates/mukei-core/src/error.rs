//! Mukei error type — TRD §1.5 / §13 / PRD §24-26.
//!
//! `MukeiError` is the **single** error enum crossing the FFI boundary.
//! Each variant maps to a stable `ERR_*` code that QML can localise and
//! render through the editorial-luxury component library. Variants are
//! intentionally thin on payload — embedded context must be redacted
//! before crossing the bridge because QML surfaces them verbatim to
//! accessibility tooling.
//!
//! New variants MUST be appended (never renumbered) and matched exhaustively
//! in [`MukeiError::error_code`].
use thiserror::Error;

/// Top-level error enum — every `Result<_, MukeiError>` in the workspace
/// converges to this type before crossing any FFI.
#[derive(Error, Debug, Clone)]
pub enum MukeiError {
    // ------------------------------------------------------------------
    // FFI / Bridge (TRD §1.3, §1.5)
    // ------------------------------------------------------------------
    #[error("FFI panic at bridge boundary")]
    FFIPanic,
    #[error("callback guard generation mismatch — QObject was destroyed")]
    CallbackGuardExpired,
    #[error("spawn_blocking task join failed: {0}")]
    BlockingJoinFailed(String),

    // ------------------------------------------------------------------
    // Resource exhaustion (TRD §38)
    // ------------------------------------------------------------------
    #[error("out of memory during inference")]
    OOM,
    #[error("memory preflight refused model load: {0}")]
    MemoryPreflightRejected(String),
    #[error("thermal throttling triggered — generation suspended")]
    ThermalThrottle,

    // ------------------------------------------------------------------
    // LLM / Inference (TRD §3)
    // ------------------------------------------------------------------
    #[error("model load failed: {0}")]
    ModelLoadFailed(String),
    #[error("SHA256 mismatch — model file corrupt or replaced")]
    ModelCorrupted,
    #[error("context creation failed: {0}")]
    ContextCreationFailed(String),
    #[error("context window overflow ({0} tokens > limit)")]
    ContextOverflow(usize),
    #[error("grammar (GBNF) load failed: {0}")]
    GrammarLoadFailed(String),

    // ------------------------------------------------------------------
    // Storage (BS §2 / TRD §6)
    // ------------------------------------------------------------------
    #[error("database initialisation failed: {0}")]
    DatabaseInitFailed(String),
    #[error("database corruption detected")]
    DatabaseCorruption,
    #[error("migration failed at version {0}: {1}")]
    MigrationFailed(u32, String),
    #[error("migration order conflict: expected {expected}, applied {applied:?}")]
    MigrationOrderConflict {
        expected: u32,
        applied:  Vec<u32>,
    },

    // ------------------------------------------------------------------
    // Config (TRD §12.5)
    // ------------------------------------------------------------------
    #[error("config.toml missing required field: {0}")]
    ConfigMissingField(String),
    #[error("config.toml field '{field}' has invalid value: {reason}")]
    ConfigInvalid {
        field:  String,
        reason: String,
    },
    #[error("config.toml contains unknown field: {0}")]
    ConfigUnknownField(String),
    #[error("safe-storage (Keystore) unavailable: {0}")]
    SafeStorageUnavailable(String),

    // ------------------------------------------------------------------
    // Crypto / Secrets (TRD §12.3)
    // ------------------------------------------------------------------
    #[error("wrapped-key envelope malformed: {0}")]
    WrappedKeyMalformed(String),
    #[error("wrapping key could not unwrap — Android Keystore failure")]
    UnwrapFailed,
    /// Tripwire variant: signals that a code path almost handed a plaintext
    /// secret to the FFI / log layer. The payload is **deliberately not a
    /// `String`** — we only carry a redacted byte-length so the error itself
    /// can never become an exfiltration channel.
    ///
    /// Construction is restricted to [`MukeiError::secret_leaked`] which
    /// zeroes the input before recording the length. Direct construction is
    /// discouraged (and grepped against in CI).
    #[error("plaintext secret would have crossed this boundary (redacted, {0} bytes)")]
    SecretLeaked(usize),

    // ------------------------------------------------------------------
    // Agent / Tool execution (TRD §2.3, §2.5, §13.3)
    // ------------------------------------------------------------------
    #[error("tool loop detected — aborted after {0} iterations")]
    ToolLoopDetected(usize),
    #[error("tool execution timeout after {0:?}")]
    ToolTimeout(Option<std::time::Duration>),
    #[error("tool '{tool_name}' is not in the allowed registry")]
    UnknownTool { tool_name: String },
    #[error("tool '{tool_name}' validator rejected payload: {reason}")]
    ToolArgsRejected {
        tool_name: String,
        reason:     String,
    },
    #[error("tool '{tool_name}' blocked — fingerprint abuse limit reached")]
    ToolAbuseBlocked { tool_name: String },
    #[error("tool '{tool_name}' is permanently disabled for this session")]
    ToolPermanentlyDisabled { tool_name: String },
    #[error("tool payload could not be parsed: {0}")]
    ToolParseFailed(String),
    #[error("tool argument '{field}' invalid: {reason}")]
    ToolArgumentInvalid { field: &'static str, reason: String },
    #[error("tool execution failed: {0}")]
    ToolExecutionFailed(String),
    #[error("web search failed: {0}")]
    WebSearchFailed(String),
    #[error("http client construction failed: {0}")]
    HttpClientFailed(String),
    #[error("file read failed: {0}")]
    FileReadFailed(String),
    #[error("binary file rejected by text-only reader")]
    BinaryFile,
    #[error("sandbox violation")]
    SandboxViolation,

    // ------------------------------------------------------------------
    // Permission / OS (TRD §14.1, BS §15)
    // ------------------------------------------------------------------
    #[error("permission denied by user or OS")]
    PermissionDenied,
    #[error("SAF URI grant revoked mid-operation")]
    SafRevoked,
    #[error("anon-FS path rejected — SAF picker is mandatory")]
    SafRequired,

    // ------------------------------------------------------------------
    // Network (TRD §5, §37.2)
    // ------------------------------------------------------------------
    #[error("network request failed: {0}")]
    NetworkError(String),
    #[error("io failure: {0}")]
    Io(String),
    #[error("resumable download hash mismatch on truncated resume")]
    DownloadHashMismatch,

    // ------------------------------------------------------------------
    // Domain-specific / diagnostics
    // ------------------------------------------------------------------
    #[error("system prompt leakage detected — aborting generation")]
    PromptLeakage,
    #[error("watchdog exceeded {kind} budget")]
    WatchdogExceeded {
        kind: &'static str,    // "seconds" | "tokens" | "memory"
    },
    #[error("FMEA fingerprint hit — operation previously crashed with this fingerprint")]
    CrashLoopDetected { fingerprint: String },
    #[error("operation cancelled by user / OS")]
    Cancelled,
    #[error("invariant violated: {0}")]
    Invariant(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl MukeiError {
    /// Stable, ASCII-only error code used by the QML side.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::FFIPanic                       => "ERR_FFI_PANIC",
            Self::CallbackGuardExpired           => "ERR_CALLBACK_GUARD_EXPIRED",
            Self::BlockingJoinFailed(_)          => "ERR_BLOCKING_JOIN",

            Self::OOM                            => "ERR_OOM",
            Self::MemoryPreflightRejected(_)     => "ERR_MEM_PREFLIGHT",
            Self::ThermalThrottle                => "ERR_THERMAL",

            Self::ModelLoadFailed(_)             => "ERR_MODEL_LOAD",
            Self::ModelCorrupted                 => "ERR_MODEL_CORRUPTED",
            Self::ContextCreationFailed(_)       => "ERR_CONTEXT_CREATE",
            Self::ContextOverflow(_)             => "ERR_CONTEXT_OVERFLOW",
            Self::GrammarLoadFailed(_)           => "ERR_GRAMMAR_LOAD",

            Self::DatabaseInitFailed(_)          => "ERR_DB_INIT",
            Self::DatabaseCorruption             => "ERR_DB_CORRUPTION",
            Self::MigrationFailed(_, _)          => "ERR_MIGRATION",
            Self::MigrationOrderConflict { .. }  => "ERR_MIGRATION_ORDER",

            Self::ConfigMissingField(_)          => "ERR_CONFIG_MISSING",
            Self::ConfigInvalid { .. }           => "ERR_CONFIG_INVALID",
            Self::ConfigUnknownField(_)          => "ERR_CONFIG_UNKNOWN",
            Self::SafeStorageUnavailable(_)      => "ERR_SAFE_STORAGE",

            Self::WrappedKeyMalformed(_)         => "ERR_WRAPPED_KEY",
            Self::UnwrapFailed                   => "ERR_UNWRAP_FAILED",
            Self::SecretLeaked(_)                => "ERR_SECRET_LEAKED",

            Self::ToolLoopDetected(_)            => "ERR_TOOL_LOOP",
            Self::ToolTimeout(_)                 => "ERR_TOOL_TIMEOUT",
            Self::UnknownTool { .. }             => "ERR_TOOL_UNKNOWN",
            Self::ToolArgsRejected { .. }        => "ERR_TOOL_ARGS",
            Self::ToolAbuseBlocked { .. }        => "ERR_TOOL_ABUSE",
            Self::ToolPermanentlyDisabled { .. } => "ERR_TOOL_DISABLED",
            Self::ToolParseFailed(_)             => "ERR_TOOL_PARSE",
            Self::ToolArgumentInvalid { .. }     => "ERR_TOOL_ARGUMENT",
            Self::ToolExecutionFailed(_)         => "ERR_TOOL_EXEC",
            Self::WebSearchFailed(_)             => "ERR_WEB_SEARCH",
            Self::HttpClientFailed(_)            => "ERR_HTTP_CLIENT",
            Self::FileReadFailed(_)              => "ERR_FILE_READ",
            Self::BinaryFile                     => "ERR_BINARY_FILE",
            Self::SandboxViolation               => "ERR_SANDBOX",

            Self::PermissionDenied               => "ERR_PERMISSION_DENIED",
            Self::SafRevoked                     => "ERR_SAF_REVOKED",
            Self::SafRequired                    => "ERR_SAF_REQUIRED",

            Self::NetworkError(_)                => "ERR_NETWORK",
            Self::Io(_)                          => "ERR_IO",
            Self::DownloadHashMismatch           => "ERR_DOWNLOAD_HASH",

            Self::PromptLeakage                  => "ERR_PROMPT_LEAKAGE",
            Self::WatchdogExceeded { .. }        => "ERR_WATCHDOG",
            Self::CrashLoopDetected { .. }       => "ERR_CRASH_LOOP",
            Self::Cancelled                      => "ERR_CANCELLED",
            Self::Invariant(_)                    => "ERR_INVARIANT",
            Self::Internal(_)                    => "ERR_INTERNAL",
        }
    }

    /// Classify the error for telemetry-free diagnostics tracking.
    /// Used by the Bloom filter (§12.1) and crash-loop prevention (§36.1).
    pub fn classification(&self) -> ErrorClass {
        match self {
            Self::OOM | Self::MemoryPreflightRejected(_) => ErrorClass::Resource,
            Self::ThermalThrottle | Self::WatchdogExceeded { .. } => ErrorClass::Device,
            Self::ModelCorrupted | Self::ModelLoadFailed(_) | Self::GrammarLoadFailed(_) => {
                ErrorClass::Inference
            }
            Self::ContextOverflow(_) | Self::ContextCreationFailed(_) => ErrorClass::Inference,
            Self::DatabaseCorruption | Self::DatabaseInitFailed(_) | Self::MigrationFailed(_, _) | Self::MigrationOrderConflict { .. } => ErrorClass::Storage,
            Self::ConfigInvalid { .. } | Self::ConfigMissingField(_) | Self::ConfigUnknownField(_) => ErrorClass::Config,
            Self::ToolLoopDetected(_) | Self::ToolTimeout(_) | Self::ToolAbuseBlocked { .. } | Self::ToolPermanentlyDisabled { .. } | Self::UnknownTool { .. } | Self::ToolArgsRejected { .. } | Self::ToolParseFailed(_) | Self::ToolArgumentInvalid { .. } | Self::ToolExecutionFailed(_) | Self::WebSearchFailed(_) | Self::HttpClientFailed(_) | Self::FileReadFailed(_) | Self::BinaryFile | Self::SandboxViolation => ErrorClass::Agent,
            Self::SafRevoked | Self::SafRequired | Self::PermissionDenied => ErrorClass::Permission,
            Self::NetworkError(_) | Self::DownloadHashMismatch | Self::Io(_) => ErrorClass::Network,
            Self::SecretLeaked(_) | Self::UnwrapFailed | Self::WrappedKeyMalformed(_) | Self::SafeStorageUnavailable(_) => ErrorClass::Security,
            _ => ErrorClass::Unknown,
        }
    }
}

/// High-level bucket used by the failure-mode tracker (§2.5 / §36.1).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    Resource,
    Device,
    Inference,
    Storage,
    Config,
    Agent,
    Permission,
    Network,
    Security,
    Unknown,
}

impl std::fmt::Display for ErrorClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Resource   => "resource",
            Self::Device     => "device",
            Self::Inference  => "inference",
            Self::Storage    => "storage",
            Self::Config     => "config",
            Self::Agent      => "agent",
            Self::Permission => "permission",
            Self::Network    => "network",
            Self::Security   => "security",
            Self::Unknown    => "unknown",
        };
        f.write_str(s)
    }
}

// ====================================================================
// Convenience type alias.
// ====================================================================
pub type Result<T> = std::result::Result<T, MukeiError>;

impl MukeiError {
    /// Tripwire constructor for [`MukeiError::SecretLeaked`].
    ///
    /// Takes ownership of a plaintext-secret-bearing `String`, zeroises its
    /// backing buffer, drops it, and records only the redacted byte length.
    /// This is the **only** sanctioned construction site for the variant —
    /// downstream code must call this helper instead of building the variant
    /// directly, otherwise the error itself becomes an exfiltration channel.
    pub fn secret_leaked(mut plaintext: String) -> Self {
        // SAFETY: zeroize the bytes BEFORE drop so a heap-inspecting attacker
        // (or a panic-handler core dump) cannot recover the secret.
        use zeroize::Zeroize;
        let len = plaintext.len();
        unsafe { plaintext.as_mut_vec().zeroize(); }
        drop(plaintext);
        Self::SecretLeaked(len)
    }
}

// ====================================================================
// Tests
// ====================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_ascii() {
        let samples = [
            MukeiError::FFIPanic,
            MukeiError::OOM,
            MukeiError::ThermalThrottle,
            MukeiError::ToolTimeout(None),
            MukeiError::PromptLeakage,
            MukeiError::UnknownTool { tool_name: "x".into() },
            MukeiError::ConfigInvalid {
                field: "models_dir".into(),
                reason: "missing".into(),
            },
        ];
        for err in samples {
            let code = err.error_code();
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "code {code} must be ASCII UPPER+UNDERSCORE"
            );
            assert!(code.starts_with("ERR_"));
        }
    }

    #[test]
    fn display_does_not_leak_secrets() {
        let err = MukeiError::secret_leaked(String::from("sk-test-123"));
        let rendered = format!("{err}");
        assert!(rendered.contains("plaintext secret"));
        assert!(!rendered.contains("sk-test-123")); // never render raw secret
        // The error MUST carry only the redacted length, never the bytes.
        assert!(matches!(err, MukeiError::SecretLeaked(n) if n == "sk-test-123".len()));
    }

    #[test]
    fn classification_is_consistent() {
        assert_eq!(
            MukeiError::OOM.classification(),
            ErrorClass::Resource
        );
        assert_eq!(
            MukeiError::ToolLoopDetected(5).classification(),
            ErrorClass::Agent
        );
    }
}
