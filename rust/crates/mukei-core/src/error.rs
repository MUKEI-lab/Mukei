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
    /// A Rust panic was caught at the FFI boundary by `catch_unwind`.
    #[error("FFI panic at bridge boundary")]
    FFIPanic,
    /// The callback's `CallbackGuard` generation no longer matches the
    /// guard's live counter — the owning QObject has been destroyed.
    #[error("callback guard generation mismatch — QObject was destroyed")]
    CallbackGuardExpired,
    /// A `tokio::task::spawn_blocking` task returned a join error
    /// (panic or cancellation inside the blocking pool).
    #[error("spawn_blocking task join failed: {0}")]
    BlockingJoinFailed(String),

    // ------------------------------------------------------------------
    // Resource exhaustion (TRD §38)
    // ------------------------------------------------------------------
    /// Out-of-memory during inference (KV cache or activation tensor).
    #[error("out of memory during inference")]
    OOM,
    /// Memory preflight check rejected the model load before mmap.
    #[error("memory preflight refused model load: {0}")]
    MemoryPreflightRejected(String),
    /// SoC thermal sensor reported throttle — generation suspended.
    #[error("thermal throttling triggered — generation suspended")]
    ThermalThrottle,

    // ------------------------------------------------------------------
    // LLM / Inference (TRD §3)
    // ------------------------------------------------------------------
    /// Underlying `llama.cpp` model load failed (mmap, GGUF parse, etc.).
    #[error("model load failed: {0}")]
    ModelLoadFailed(String),
    /// Header / file SHA-256 did not match the pinned value (REQ-SEC-01).
    #[error("SHA256 mismatch — model file corrupt or replaced")]
    ModelCorrupted,
    /// Could not construct a llama context (n_ctx too large, OOM, etc.).
    #[error("context creation failed: {0}")]
    ContextCreationFailed(String),
    /// The assembled prompt exceeded the active n_ctx budget.
    #[error("context window overflow ({0} tokens > limit)")]
    ContextOverflow(usize),
    /// GBNF grammar file failed to load or parse.
    #[error("grammar (GBNF) load failed: {0}")]
    GrammarLoadFailed(String),

    // ------------------------------------------------------------------
    // Storage (BS §2 / TRD §6)
    // ------------------------------------------------------------------
    /// Could not open the SQLite database / build the r2d2 pool.
    #[error("database initialisation failed: {0}")]
    DatabaseInitFailed(String),
    /// SQLite reported a corruption sentinel (`SQLITE_CORRUPT`).
    #[error("database corruption detected")]
    DatabaseCorruption,
    /// A migration script failed at the indicated version.
    #[error("migration failed at version {0}: {1}")]
    MigrationFailed(u32, String),
    /// The `migrations_applied` table shows an out-of-order set; boot
    /// refuses to start to avoid silently skipping a schema bump.
    #[error("migration order conflict: expected {expected}, applied {applied:?}")]
    MigrationOrderConflict {
        /// The next version the boot path tried to apply.
        expected: u32,
        /// The full applied set as found on disk.
        applied:  Vec<u32>,
    },

    // ------------------------------------------------------------------
    // Config (TRD §12.5)
    // ------------------------------------------------------------------
    /// A required top-level key was absent from `config.toml`.
    #[error("config.toml missing required field: {0}")]
    ConfigMissingField(String),
    /// A `config.toml` field is present but holds an illegal value.
    #[error("config.toml field '{field}' has invalid value: {reason}")]
    ConfigInvalid {
        /// Dotted field path (e.g. `watchdog.max_iterations`).
        field:  String,
        /// Human-readable reason; surfaced to QML for the error dialog.
        reason: String,
    },
    /// A `config.toml` root key was not on the strict allow-list.
    #[error("config.toml contains unknown field: {0}")]
    ConfigUnknownField(String),
    /// Android Keystore / desktop keyring is unavailable; secrets cannot
    /// be unwrapped this boot.
    #[error("safe-storage (Keystore) unavailable: {0}")]
    SafeStorageUnavailable(String),

    // ------------------------------------------------------------------
    // Crypto / Secrets (TRD §12.3)
    // ------------------------------------------------------------------
    /// A wrapped-key blob from disk did not decode to a valid envelope.
    #[error("wrapped-key envelope malformed: {0}")]
    WrappedKeyMalformed(String),
    /// The wrapping key existed but `unwrap_key` failed at Keystore.
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
    /// The ReAct loop made more tool-call iterations than the watchdog
    /// budget allows. Carries the iteration count at abort.
    #[error("tool loop detected — aborted after {0} iterations")]
    ToolLoopDetected(usize),
    /// A tool exceeded its per-call timeout. `None` means the timeout was
    /// hit but the configured duration is not exposed for telemetry-free
    /// diagnostics.
    #[error("tool execution timeout after {0:?}")]
    ToolTimeout(Option<std::time::Duration>),
    /// The LLM emitted a tool name absent from `ALLOWED_TOOLS`.
    #[error("tool '{tool_name}' is not in the allowed registry")]
    UnknownTool {
        /// Tool name as emitted by the LLM.
        tool_name: String,
    },
    /// The post-GBNF validator rejected the arguments for this tool.
    #[error("tool '{tool_name}' validator rejected payload: {reason}")]
    ToolArgsRejected {
        /// Tool name.
        tool_name: String,
        /// Validator's human-readable rejection reason; re-fed to the LLM.
        reason:     String,
    },
    /// The same tool failed twice with the same argument fingerprint
    /// (REQ-AGT-04). The agent loop downgrades to graceful degrade.
    #[error("tool '{tool_name}' blocked — fingerprint abuse limit reached")]
    ToolAbuseBlocked {
        /// Tool name that hit the abuse threshold.
        tool_name: String,
    },
    /// User / config disabled this tool for the entire session.
    #[error("tool '{tool_name}' is permanently disabled for this session")]
    ToolPermanentlyDisabled {
        /// Tool name.
        tool_name: String,
    },
    /// Could not parse the LLM output into a tool-call payload at all.
    #[error("tool payload could not be parsed: {0}")]
    ToolParseFailed(String),
    /// A specific tool argument failed its semantic validation.
    #[error("tool argument '{field}' invalid: {reason}")]
    ToolArgumentInvalid {
        /// Argument field name (static string).
        field: &'static str,
        /// Reason for rejection.
        reason: String,
    },
    /// The tool ran but reported a runtime failure.
    #[error("tool execution failed: {0}")]
    ToolExecutionFailed(String),
    /// All web-search backends failed or returned zero results.
    #[error("web search failed: {0}")]
    WebSearchFailed(String),
    /// The HTTP client could not be constructed (TLS / DNS init).
    #[error("http client construction failed: {0}")]
    HttpClientFailed(String),
    /// `read_file` could not read the resolved SAF target.
    #[error("file read failed: {0}")]
    FileReadFailed(String),
    /// `read_file` refused a non-UTF-8 file (binary heuristic).
    #[error("binary file rejected by text-only reader")]
    BinaryFile,
    /// A canonicalised path escaped the jail root (path traversal block).
    #[error("sandbox violation")]
    SandboxViolation,

    // ------------------------------------------------------------------
    // Permission / OS (TRD §14.1, BS §15)
    // ------------------------------------------------------------------
    /// The user or OS denied a required permission.
    #[error("permission denied by user or OS")]
    PermissionDenied,
    /// The SAF URI grant was revoked while the tool was using it.
    #[error("SAF URI grant revoked mid-operation")]
    SafRevoked,
    /// A tool refused to operate on a non-SAF path (file picker required).
    #[error("anon-FS path rejected — SAF picker is mandatory")]
    SafRequired,

    // ------------------------------------------------------------------
    // Network (TRD §5, §37.2)
    // ------------------------------------------------------------------
    /// A network request failed at the transport layer.
    #[error("network request failed: {0}")]
    NetworkError(String),
    /// Generic I/O failure (file system, network adapter).
    #[error("io failure: {0}")]
    Io(String),
    /// A resumable download saw a SHA-256 mismatch on truncated resume.
    #[error("resumable download hash mismatch on truncated resume")]
    DownloadHashMismatch,

    // ------------------------------------------------------------------
    // Domain-specific / diagnostics
    // ------------------------------------------------------------------
    /// The system prompt was detected in the model output — abort to
    /// avoid leaking the agent's identity (PRD §12).
    #[error("system prompt leakage detected — aborting generation")]
    PromptLeakage,
    /// One of the watchdog budgets (iterations / tokens / wall-time)
    /// was exhausted.
    #[error("watchdog exceeded {kind} budget")]
    WatchdogExceeded {
        /// Which budget tripped: `"seconds"`, `"tokens"`, or `"iterations"`.
        kind: &'static str,
    },
    /// The crash-loop tripwire matched a fingerprint that has crashed
    /// before — boot refuses to retry the same code path automatically.
    #[error("FMEA fingerprint hit — operation previously crashed with this fingerprint")]
    CrashLoopDetected {
        /// SHA-256 hex of the crash signature.
        fingerprint: String,
    },
    /// The user (or OS lifecycle) cancelled this operation.
    #[error("operation cancelled by user / OS")]
    Cancelled,
    /// A code-internal invariant assertion failed. Should be unreachable.
    #[error("invariant violated: {0}")]
    Invariant(String),
    /// Catch-all for unclassified internal errors; prefer a typed variant.
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
            Self::SecretLeaked(_) | Self::UnwrapFailed | Self::WrappedKeyMalformed(_) | Self::SafeStorageUnavailable(_) | Self::PromptLeakage => ErrorClass::Security,
            // Issue #19: the previous `_ => Unknown` wildcard let new error
            // variants silently land in Unknown. We list every remaining
            // variant explicitly so the compiler enforces classification
            // for any future variant via E0004 (non-exhaustive match).
            Self::FFIPanic | Self::CallbackGuardExpired | Self::BlockingJoinFailed(_) => ErrorClass::Resource,
            Self::CrashLoopDetected { .. } => ErrorClass::Device,
            Self::Cancelled | Self::Invariant(_) | Self::Internal(_) => ErrorClass::Unknown,
        }
    }
}

/// High-level bucket used by the failure-mode tracker (§2.5 / §36.1).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    /// OOM, memory preflight refusals.
    Resource,
    /// Thermal throttle, watchdog wall-time / iteration budget hit.
    Device,
    /// llama.cpp model load / context / grammar failures.
    Inference,
    /// SQLite / migrations / corruption.
    Storage,
    /// `config.toml` validation errors.
    Config,
    /// Tool execution + validation failures.
    Agent,
    /// SAF / OS permission rejections.
    Permission,
    /// Network-layer faults (HTTP, DNS, transport).
    Network,
    /// Crypto / wrapped-key / secret-leak tripwires.
    Security,
    /// Catch-all when no other class applies.
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
/// Workspace-wide `Result` shorthand — every fallible function in
/// `mukei-core` returns `Result<T> = std::result::Result<T, MukeiError>`.
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
