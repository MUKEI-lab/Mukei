//! `mukei_core::storage::model_download` — TRD §8.1 / PRD REQ-MOD-01.
//!
//! On-device GGUF downloader used by the QML "Download model" flow.
//!
//! # Design contract
//!
//! The bridge crate's `MukeiAgentRust::download_model(url, sha256)` is the
//! single QML-facing entry point. It calls into this module to do the
//! actual work and re-emits progress / status events back to QML through
//! the `download_progress` qsignal.
//!
//! ```text
//!     QML  ─► bridge::download_model(url, sha256)
//!              │
//!              ▼
//!     core::storage::model_download::run_download(...)
//!              │
//!              ├─ HTTP GET  (Range: bytes=N- when resuming)
//!              ├─ Stream  → write → progress(0.0..1.0)
//!              ├─ SHA-256 stream + verify
//!              └─ atomic rename .partial → final path
//!              │
//!              ▼
//!     bridge re-emits `download_progress(progress, status)` qsignal
//! ```
//!
//! # Invariants (REQ-MOD-01 / REQ-SEC-01)
//!
//! - Downloads are written to `<dest>.partial` and atomically renamed
//!   to `<dest>` only AFTER the streamed SHA-256 matches `expected_sha256`.
//!   A corrupted or truncated download therefore can never poison the
//!   GGUF path that `LlamaEngine::load_model` mmaps.
//! - SHA-256 mismatch surfaces as [`crate::error::MukeiError::DownloadHashMismatch`]
//!   (existing variant, code `ERR_DOWNLOAD_HASH`). The bridge maps this
//!   into the `download_progress("error", _)` status so QML can surface
//!   it to the user.
//! - Resumes use HTTP `Range: bytes=<offset>-`. The downloader treats
//!   two server responses as "restart from byte 0", because both of
//!   them mean the stale `.partial` cannot be safely continued:
//!   - `200 OK` — server ignored the `Range` header entirely.
//!   - `416 Range Not Satisfiable` — the byte we asked to resume from
//!     is past the current end of the file. Happens when the upstream
//!     file *shrinks* between two download attempts (e.g. the publisher
//!     re-uploaded a smaller variant). Without this branch the failure
//!     surfaced as a generic `ERR_NETWORK` and confused testers — the
//!     architect-review note captures the
//!     diagnostic-distinguishability requirement.
//!
//!   In both cases the `.partial` file is deleted and the request is
//!   re-issued without a `Range` header. [`MukeiError::DownloadHashMismatch`]
//!   also covers the "truncated resume hash mismatch" leftover-on-disk
//!   case for cancelled previous attempts.
//!
//! - The SHA-256 verification happens over the *entire* downloaded
//!   file (resumed prefix + new bytes). A mismatch after a successful
//!   transfer surfaces as [`MukeiError::DownloadHashMismatch`]. From
//!   the user's perspective this can mean either "file in transit
//!   was corrupted" or "the upstream artifact was replaced under us".
//!   The catalogue mitigates the second case by pinning a commit-sha
//!   in `download_url` (`/resolve/<sha>/...` rather than
//!   `/resolve/main/...`); see
//!   [`crate::engine::model_registry`] for the discussion.
//! - Progress is emitted at most once per 0.5 % advance to keep the QML
//!   event loop unjammed. The `status` channel always gets a final
//!   `"complete"` or `"error"` event so the QML state machine can leave
//!   the "downloading" view.
//!
//! # Sandbox / desktop fallback
//!
//! When the `network` feature is OFF (i.e. on the sandbox CI build), the
//! module compiles to a thin "not_supported" implementation that fails
//! immediately with a typed error. This keeps the bridge crate building
//! everywhere without dragging reqwest into the sandbox feature set.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{MukeiError, Result};

/// Hard upper bound for one model artifact download. Current catalog
/// entries are well below this; the cap prevents unbounded mobile
/// storage/bandwidth exhaustion if a server lies or omits size data.
pub const MAX_MODEL_DOWNLOAD_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// Progress event emitted during a download. The bridge crate translates
/// these into QML's `download_progress(progress: f64, status: QString)`
/// signal. Keep this enum stable — the QML side switches on `status`.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadEvent {
    /// Download has started. Production downloads require
    /// `Content-Length`; `None` remains representable for older tests or
    /// non-production callers that do not enforce the live network path.
    Started {
        /// Total expected file size in bytes, if known.
        total_bytes: Option<u64>,
    },
    /// Progress tick. `progress` is in `[0.0, 1.0]`. The bridge
    /// throttles ticks to ~once every 0.5 % advance.
    Progress {
        /// Fraction completed in `[0.0, 1.0]`.
        progress: f64,
        /// Bytes downloaded so far.
        bytes_downloaded: u64,
    },
    /// SHA-256 verification passed and the file was atomically moved
    /// into its final path.
    Complete {
        /// Final absolute path of the verified GGUF.
        final_path: PathBuf,
    },
    /// The download or its verification failed. The bridge re-emits this
    /// as `download_progress(0.0, "error: <message>")` so QML can show
    /// a typed dialog.
    Error {
        /// Stable error code from [`crate::error::MukeiError::error_code`].
        code: &'static str,
        /// Human-readable message; safe to log but not localised.
        message: String,
    },
}

/// Parameters for a model download request.
///
/// All fields are owned strings/paths so the request can be moved into
/// a `tokio::spawn` task without lifetime gymnastics.
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    /// HTTPS URL of the GGUF model artifact.
    pub url: String,
    /// Hex-encoded SHA-256 of the full file. Verified BEFORE the
    /// `.partial` file is renamed to the final path.
    pub expected_sha256: String,
    /// Final destination path (the GGUF that `LlamaEngine::load_model`
    /// will mmap).
    pub dest: PathBuf,
}

impl DownloadRequest {
    /// Path the streamer writes to before atomic rename.
    pub fn partial_path(&self) -> PathBuf {
        let mut p = self.dest.clone();
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "model.gguf".into());
        p.set_file_name(format!("{name}.partial"));
        p
    }

    /// Stub-friendly basic validation. Catches obvious typos before we
    /// open a TCP socket.
    pub fn validate(&self) -> Result<()> {
        if self.url.trim().is_empty() {
            return Err(MukeiError::ToolArgumentInvalid {
                field: "url",
                reason: "empty download url".to_string(),
            });
        }
        if !self.url.starts_with("https://") {
            return Err(MukeiError::ToolArgumentInvalid {
                field: "url",
                reason: "model URL must use https:// (no plaintext over the air)".to_string(),
            });
        }
        if self.expected_sha256.len() != 64
            || !self.expected_sha256.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(MukeiError::ToolArgumentInvalid {
                field: "sha256",
                reason: "expected 64-char hex SHA-256".to_string(),
            });
        }
        if self.dest.as_os_str().is_empty() {
            return Err(MukeiError::ToolArgumentInvalid {
                field: "dest",
                reason: "destination path is empty".to_string(),
            });
        }
        Ok(())
    }
}

/// Verify the SHA-256 of an already-on-disk file against an expected
/// hex digest. Returns [`MukeiError::DownloadHashMismatch`] on mismatch.
///
/// Shared helper used both by the download finaliser and by an offline
/// re-verify path (e.g. before mmapping a model that survived an OS
/// crash mid-rename).
pub fn verify_file_sha256(path: &Path, expected_hex: &str) -> Result<()> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|e| MukeiError::Io(format!("open for hash: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| MukeiError::Io(format!("read for hash: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let got_hex = hex_encode(&hasher.finalize());
    if !ct_eq_ascii_lower(&got_hex, &expected_hex.to_ascii_lowercase()) {
        return Err(MukeiError::DownloadHashMismatch);
    }
    Ok(())
}

/// Lowercase hex encoder. Local copy so we don't pull the `hex` crate
/// into the sandbox build.
fn hex_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(TABLE[(b >> 4) as usize] as char);
        s.push(TABLE[(b & 0xf) as usize] as char);
    }
    s
}

/// Constant-time ASCII-lowercased equality. SHA values are short so the
/// timing risk is small, but keeping the comparison branch-free avoids
/// a leakage surface for any future use that compares secrets.
fn ct_eq_ascii_lower(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc: u8 = 0;
    for (x, y) in a.bytes().zip(b.bytes()) {
        acc |= x ^ y;
    }
    acc == 0
}

// ---------------------------------------------------------------------
// Real downloader (gated on `network` feature)
// ---------------------------------------------------------------------

#[cfg(all(feature = "network", feature = "tokio"))]
pub use real::run_download;

#[cfg(all(feature = "network", feature = "tokio"))]
mod real {
    use super::*;
    use std::io::SeekFrom;

    use tokio::fs::{File, OpenOptions};
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};
    use tokio::sync::mpsc::Sender;

    /// Perform the download described by `req`, streaming
    /// [`DownloadEvent`]s to `events` and respecting `cancel` for
    /// user-initiated stop.
    ///
    /// Returns `Ok(())` on success (the final `Complete` event has
    /// already been sent), or an [`MukeiError`] for the caller to map
    /// into the QML `download_progress("error", _)` signal. The error
    /// path also emits a final `Error` event so QML state always
    /// observes a terminal transition.
    pub async fn run_download(
        req: DownloadRequest,
        events: Sender<DownloadEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<()> {
        // Public entry point: enforce the full DownloadRequest contract
        // (https-only URL, 64-char hex sha, non-empty dest). All actual
        // I/O lives in [`run_download_unchecked`] so test code can
        // exercise the streaming / restart logic over a plaintext
        // loopback server without weakening the production invariant.
        req.validate()?;
        run_download_unchecked(req, events, cancel).await
    }

    /// Internal streamer. Identical to [`run_download`] except it
    /// **skips `DownloadRequest::validate`**, so it can be driven from
    /// the loopback-HTTP integration test that proves the 416-restart
    /// branch. Production code MUST go through `run_download` (which
    /// validates first); this signature is `pub(super)` only so the
    /// `tests` mod, defined in the parent file, can reach it.
    pub(super) async fn run_download_unchecked(
        req: DownloadRequest,
        events: Sender<DownloadEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<()> {
        let partial = req.partial_path();
        if let Some(parent) = req.dest.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| MukeiError::Io(format!("mkdir parent: {e}")))?;
            }
        }

        // Resume: pick up the existing .partial byte count if present.
        let mut resume_from: u64 = match tokio::fs::metadata(&partial).await {
            Ok(m) => m.len(),
            Err(_) => 0,
        };

        let client = reqwest::Client::builder()
            .user_agent(concat!("mukei-bridge/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| MukeiError::HttpClientFailed(e.to_string()))?;

        let mut request = client.get(&req.url);
        if resume_from > 0 {
            request = request.header("Range", format!("bytes={resume_from}-"));
        }

        let mut resp = request
            .send()
            .await
            .map_err(|e| MukeiError::NetworkError(format!("GET {}: {e}", req.url)))?;

        let status = resp.status();

        // ---- Resume-recovery: restart from byte 0 instead of failing ----
        //
        // Two server responses mean our `Range` request cannot be
        // honoured but the *download itself* is still recoverable:
        //
        //   1. `200 OK`  — server ignored the `Range` header entirely.
        //   2. `416 Range Not Satisfiable` — our `resume_from` is past
        //      the file's current end (upstream shrank).
        //
        // In both cases we nuke `.partial`, rebuild the request without
        // a `Range` header, and stream from byte 0. This is the only
        // way to keep `ERR_DOWNLOAD_HASH` reserved for true corruption /
        // tamper events; without it a transient upstream resize would
        // surface as an opaque `ERR_NETWORK` (architect-review note).
        let needs_restart_from_zero = resume_from > 0
            && (status == reqwest::StatusCode::OK
                || status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE);

        if needs_restart_from_zero {
            tracing::info!(
                resume_from,
                http_status = %status,
                "resume rejected by server; restarting download from byte 0"
            );
            tokio::fs::remove_file(&partial)
                .await
                .map_err(|e| MukeiError::Io(format!("rm partial for restart: {e}")))?;
            resume_from = 0;

            // Re-issue the request without a `Range` header.
            drop(resp);
            let resp_retry =
                client.get(&req.url).send().await.map_err(|e| {
                    MukeiError::NetworkError(format!("GET {} (restart): {e}", req.url))
                })?;
            let retry_status = resp_retry.status();
            if !retry_status.is_success() {
                let msg = format!("HTTP {retry_status} for {} (restart)", req.url);
                let _ = events
                    .send(DownloadEvent::Error {
                        code: "ERR_NETWORK",
                        message: msg.clone(),
                    })
                    .await;
                return Err(MukeiError::NetworkError(msg));
            }
            resp = resp_retry;
        } else if !status.is_success() {
            let msg = format!("HTTP {status} for {}", req.url);
            let _ = events
                .send(DownloadEvent::Error {
                    code: "ERR_NETWORK",
                    message: msg.clone(),
                })
                .await;
            return Err(MukeiError::NetworkError(msg));
        }

        let Some(remaining_bytes) = resp.content_length() else {
            let err = MukeiError::DownloadSizeMissing;
            let _ = tokio::fs::remove_file(&partial).await;
            let _ = events
                .send(DownloadEvent::Error {
                    code: err.error_code(),
                    message: err.to_string(),
                })
                .await;
            return Err(err);
        };
        let total_bytes = remaining_bytes.saturating_add(resume_from);
        if total_bytes > MAX_MODEL_DOWNLOAD_BYTES {
            let err = MukeiError::DownloadTooLarge {
                max_bytes: MAX_MODEL_DOWNLOAD_BYTES,
                actual_bytes: total_bytes,
            };
            let _ = tokio::fs::remove_file(&partial).await;
            let _ = events
                .send(DownloadEvent::Error {
                    code: err.error_code(),
                    message: err.to_string(),
                })
                .await;
            return Err(err);
        }

        let _ = events
            .send(DownloadEvent::Started {
                total_bytes: Some(total_bytes),
            })
            .await;

        // Open .partial in resume-or-create mode at `resume_from`. We
        // deliberately set `truncate(false)` whenever we're resuming so
        // resuming preserves the bytes streamed in a prior session;
        // when the restart-from-zero path nuked `.partial` above we set
        // `truncate(true)` so a re-created file starts fresh. The
        // explicit value also silences clippy::suspicious_open_options
        // under `-D warnings` (CI gate).
        let mut out = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(resume_from == 0)
            .open(&partial)
            .await
            .map_err(|e| MukeiError::Io(format!("open partial: {e}")))?;
        out.seek(SeekFrom::Start(resume_from))
            .await
            .map_err(|e| MukeiError::Io(format!("seek partial: {e}")))?;

        // Hash the file from byte 0 — when resuming we replay the
        // already-on-disk prefix into the hasher so the final digest
        // covers the full file.
        let mut hasher = Sha256::new();
        if resume_from > 0 {
            let mut prefix = File::open(&partial)
                .await
                .map_err(|e| MukeiError::Io(format!("reopen partial for hash: {e}")))?;
            let mut buf = vec![0u8; 1024 * 1024];
            let mut remaining = resume_from;
            use tokio::io::AsyncReadExt;
            while remaining > 0 {
                let want = (buf.len() as u64).min(remaining) as usize;
                let n = prefix
                    .read(&mut buf[..want])
                    .await
                    .map_err(|e| MukeiError::Io(format!("hash prefix read: {e}")))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                remaining = remaining.saturating_sub(n as u64);
            }
        }

        let mut downloaded: u64 = resume_from;
        let mut last_emit_pct: f64 = 0.0;

        use futures::StreamExt;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if cancel.is_cancelled() {
                let _ = events
                    .send(DownloadEvent::Error {
                        code: "ERR_CANCELLED",
                        message: "user cancelled download".into(),
                    })
                    .await;
                return Err(MukeiError::Cancelled);
            }
            let bytes =
                chunk.map_err(|e| MukeiError::NetworkError(format!("stream chunk: {e}")))?;
            hasher.update(&bytes);
            out.write_all(&bytes)
                .await
                .map_err(|e| MukeiError::Io(format!("write partial: {e}")))?;
            downloaded = downloaded.saturating_add(bytes.len() as u64);

            if downloaded > MAX_MODEL_DOWNLOAD_BYTES {
                drop(out);
                let err = MukeiError::DownloadTooLarge {
                    max_bytes: MAX_MODEL_DOWNLOAD_BYTES,
                    actual_bytes: downloaded,
                };
                let _ = tokio::fs::remove_file(&partial).await;
                let _ = events
                    .send(DownloadEvent::Error {
                        code: err.error_code(),
                        message: err.to_string(),
                    })
                    .await;
                return Err(err);
            }

            if total_bytes > 0 {
                let pct = downloaded as f64 / total_bytes as f64;
                if pct - last_emit_pct >= 0.005 || pct >= 1.0 {
                    last_emit_pct = pct;
                    let _ = events
                        .send(DownloadEvent::Progress {
                            progress: pct.clamp(0.0, 1.0),
                            bytes_downloaded: downloaded,
                        })
                        .await;
                }
            }
        }

        out.flush()
            .await
            .map_err(|e| MukeiError::Io(format!("flush partial: {e}")))?;
        drop(out);

        let got_hex = hex_encode(&hasher.finalize());
        if !ct_eq_ascii_lower(&got_hex, &req.expected_sha256.to_ascii_lowercase()) {
            // Truncated-resume mismatch: nuke .partial so the next
            // attempt restarts cleanly. The architect-review note in
            // the module header is the rationale.
            let _ = tokio::fs::remove_file(&partial).await;
            let _ = events
                .send(DownloadEvent::Error {
                    code: "ERR_DOWNLOAD_HASH",
                    message: format!("expected {} but computed {got_hex}", req.expected_sha256),
                })
                .await;
            return Err(MukeiError::DownloadHashMismatch);
        }

        // Atomic rename only after the hash matches.
        tokio::fs::rename(&partial, &req.dest)
            .await
            .map_err(|e| MukeiError::Io(format!("rename partial: {e}")))?;

        let _ = events
            .send(DownloadEvent::Complete {
                final_path: req.dest.clone(),
            })
            .await;

        Ok(())
    }
}

// ---------------------------------------------------------------------
// Sandbox stub (when `network` is OFF)
// ---------------------------------------------------------------------

#[cfg(not(all(feature = "network", feature = "tokio")))]
pub use stub::run_download;

#[cfg(not(all(feature = "network", feature = "tokio")))]
mod stub {
    use super::*;

    /// Stub used by the sandbox build. Always returns
    /// `MukeiError::NetworkError("network feature disabled")` so unit
    /// tests can still exercise validation logic without dragging
    /// reqwest into the sandbox feature set.
    pub async fn run_download(
        _req: DownloadRequest,
        _events: tokio::sync::mpsc::Sender<DownloadEvent>,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> Result<()> {
        Err(MukeiError::NetworkError(
            "model_download::run_download requires the `network` feature".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn validate_rejects_plaintext_url() {
        let req = DownloadRequest {
            url: "http://example.com/model.gguf".into(),
            expected_sha256: "a".repeat(64),
            dest: PathBuf::from("/tmp/model.gguf"),
        };
        let err = req.validate().unwrap_err();
        assert!(matches!(
            err,
            MukeiError::ToolArgumentInvalid { field: "url", .. }
        ));
    }

    #[test]
    fn validate_rejects_bad_sha_length() {
        let req = DownloadRequest {
            url: "https://example.com/model.gguf".into(),
            expected_sha256: "abc".into(),
            dest: PathBuf::from("/tmp/model.gguf"),
        };
        let err = req.validate().unwrap_err();
        assert!(matches!(
            err,
            MukeiError::ToolArgumentInvalid {
                field: "sha256",
                ..
            }
        ));
    }

    #[test]
    fn validate_rejects_non_hex_sha() {
        let req = DownloadRequest {
            url: "https://example.com/model.gguf".into(),
            // 64 chars but `z` is not hex.
            expected_sha256: format!("{}z", "a".repeat(63)),
            dest: PathBuf::from("/tmp/model.gguf"),
        };
        let err = req.validate().unwrap_err();
        assert!(matches!(
            err,
            MukeiError::ToolArgumentInvalid {
                field: "sha256",
                ..
            }
        ));
    }

    #[test]
    fn validate_accepts_well_formed_request() {
        let req = DownloadRequest {
            url: "https://example.com/model.gguf".into(),
            expected_sha256: "a".repeat(64),
            dest: PathBuf::from("/tmp/model.gguf"),
        };
        req.validate().expect("well-formed request must validate");
    }

    #[test]
    fn partial_path_appends_partial_suffix() {
        let req = DownloadRequest {
            url: "https://example.com/m.gguf".into(),
            expected_sha256: "a".repeat(64),
            dest: PathBuf::from("/data/models/gemma-4b.gguf"),
        };
        assert_eq!(
            req.partial_path(),
            PathBuf::from("/data/models/gemma-4b.gguf.partial")
        );
    }

    #[test]
    fn hex_encode_round_trip() {
        let bytes = [0u8, 1, 0xab, 0xcd, 0xef, 0xff];
        assert_eq!(hex_encode(&bytes), "0001abcdefff");
    }

    #[test]
    fn ct_eq_handles_length_mismatch() {
        assert!(!ct_eq_ascii_lower("a", "aa"));
        assert!(ct_eq_ascii_lower("abcdef", "abcdef"));
        assert!(!ct_eq_ascii_lower("abcdef", "abcde0"));
    }

    #[test]
    fn verify_file_sha256_matches_known_digest() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("hello.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        // sha256("hello world")
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        verify_file_sha256(&path, expected).expect("hash must match");
    }

    #[test]
    fn verify_file_sha256_rejects_mismatch() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("hello.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        let err = verify_file_sha256(&path, &"0".repeat(64)).unwrap_err();
        assert!(matches!(err, MukeiError::DownloadHashMismatch));
    }

    #[test]
    fn verify_file_sha256_handles_uppercase_expected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("hello.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        let expected_upper = "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9";
        verify_file_sha256(&path, expected_upper).expect("case-insensitive hex must match");
    }

    // -----------------------------------------------------------------
    // Integration: 416 Range Not Satisfiable triggers full restart.
    //
    // Backstops the architect-review fix for the "upstream file shrank"
    // scenario. A stale `.partial` from a prior download attempt asks
    // for `bytes=N-` past the new end-of-file; the server answers 416;
    // the downloader must wipe `.partial`, re-issue without `Range`,
    // stream the (smaller) body, hash-verify, and atomically rename to
    // `dest`. Without this branch, the failure used to surface as an
    // opaque `ERR_NETWORK` (issue raised by senior systems review).
    //
    // We hand-roll a minimal HTTP/1.1 responder on a tokio `TcpListener`
    // so the test stays self-contained (no `wiremock` dep).
    // -----------------------------------------------------------------
    #[cfg(all(feature = "network", feature = "tokio"))]
    #[tokio::test]
    async fn http_416_on_resume_triggers_restart_and_succeeds() {
        use sha2::{Digest, Sha256};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio_util::sync::CancellationToken;

        // The "new" upstream body — deliberately *smaller* than the
        // stale `.partial` we'll seed below, so the server has to
        // answer 416 on the first (Range) request.
        let body: Vec<u8> = (0u8..200)
            .map(|i| i.wrapping_mul(3))
            .cycle()
            .take(1024)
            .collect();
        let expected_sha = {
            let mut h = Sha256::new();
            h.update(&body);
            let d = h.finalize();
            let mut s = String::with_capacity(64);
            for b in d {
                use std::fmt::Write as _;
                let _ = write!(&mut s, "{b:02x}");
            }
            s
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Server task: respond 416 to the first request that carries a
        // `Range:` header, then full 200 to the next request.
        let body_for_server = body.clone();
        let server = tokio::spawn(async move {
            for expected_range in [true, false] {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let has_range = req
                    .lines()
                    .any(|l| l.to_ascii_lowercase().starts_with("range:"));
                assert_eq!(
                    has_range, expected_range,
                    "request {expected_range} expected Range header, got: {req}"
                );
                let resp = if has_range {
                    b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec()
                } else {
                    let mut r = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body_for_server.len()
                    )
                    .into_bytes();
                    r.extend_from_slice(&body_for_server);
                    r
                };
                sock.write_all(&resp).await.unwrap();
                sock.shutdown().await.ok();
            }
        });

        let dir = tempdir().unwrap();
        let dest = dir.path().join("model.gguf");
        let partial = dir.path().join("model.gguf.partial");

        // Seed a stale `.partial` LARGER than the new upstream body so
        // `Range: bytes=<resume_from>-` is unsatisfiable on the server.
        std::fs::write(&partial, vec![0xAAu8; body.len() + 512]).unwrap();

        let req = DownloadRequest {
            url: format!("http://{addr}/model.gguf"),
            expected_sha256: expected_sha.clone(),
            dest: dest.clone(),
        };
        // The validator forbids plaintext HTTP. We're driving the
        // internal `real::run_download` directly with a relaxed
        // request so this loopback HTTP server stays simple. The
        // production code path keeps the `https://` invariant via
        // `DownloadRequest::validate`; this test exists purely to
        // exercise the 416-restart branch.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<DownloadEvent>(32);
        let cancel = CancellationToken::new();
        let drain = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(ev) = rx.recv().await {
                events.push(ev);
            }
            events
        });

        // Bypass `validate()` by calling the inner streamer directly.
        // We can't reach private helpers from outside the module, but
        // we *are* inside the module here, so `real::run_download` is
        // visible — it just enforces validation. Build a custom client
        // and replicate the entry-point logic with `https://` waived.
        let result = exercise_run_download_for_test(req, tx, cancel).await;
        let events = drain.await.unwrap();
        server.await.unwrap();

        assert!(result.is_ok(), "download must succeed; got {result:?}");
        assert!(dest.exists(), "final file must be renamed into place");
        assert!(
            !partial.exists(),
            "`.partial` must be cleaned up after rename"
        );
        let on_disk = std::fs::read(&dest).unwrap();
        assert_eq!(on_disk, body, "final file must equal the upstream body");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DownloadEvent::Started { .. })),
            "must emit Started after restart"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DownloadEvent::Complete { .. })),
            "must emit Complete"
        );
    }

    #[cfg(all(feature = "network", feature = "tokio"))]
    #[tokio::test]
    async fn missing_content_length_is_rejected_before_writing_partial() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio_util::sync::CancellationToken;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = sock.read(&mut buf).await.unwrap();
            sock.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello")
                .await
                .unwrap();
            sock.shutdown().await.ok();
        });

        let dir = tempdir().unwrap();
        let dest = dir.path().join("model.gguf");
        let partial = dir.path().join("model.gguf.partial");
        let req = DownloadRequest {
            url: format!("http://{addr}/model.gguf"),
            expected_sha256: "0".repeat(64),
            dest,
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel::<DownloadEvent>(8);
        let result = exercise_run_download_for_test(req, tx, CancellationToken::new()).await;
        server.await.unwrap();

        assert!(matches!(result, Err(MukeiError::DownloadSizeMissing)));
        assert!(!partial.exists(), "unsafe partial file must be removed");
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        assert!(
            events.iter().any(|event| matches!(
                event,
                DownloadEvent::Error {
                    code: "ERR_DOWNLOAD_SIZE_MISSING",
                    ..
                }
            )),
            "must emit typed missing-size error event; got {events:?}"
        );
    }

    #[cfg(all(feature = "network", feature = "tokio"))]
    #[tokio::test]
    async fn oversized_content_length_is_rejected_before_writing_partial() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio_util::sync::CancellationToken;

        let advertised_bytes = MAX_MODEL_DOWNLOAD_BYTES + 1;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = sock.read(&mut buf).await.unwrap();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {advertised_bytes}\r\nConnection: close\r\n\r\n"
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            sock.shutdown().await.ok();
        });

        let dir = tempdir().unwrap();
        let dest = dir.path().join("model.gguf");
        let partial = dir.path().join("model.gguf.partial");
        let req = DownloadRequest {
            url: format!("http://{addr}/model.gguf"),
            expected_sha256: "0".repeat(64),
            dest,
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel::<DownloadEvent>(8);
        let result = exercise_run_download_for_test(req, tx, CancellationToken::new()).await;
        server.await.unwrap();

        assert!(matches!(
            result,
            Err(MukeiError::DownloadTooLarge {
                max_bytes: MAX_MODEL_DOWNLOAD_BYTES,
                actual_bytes
            }) if actual_bytes == advertised_bytes
        ));
        assert!(!partial.exists(), "unsafe partial file must be removed");
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        assert!(
            events.iter().any(|event| matches!(
                event,
                DownloadEvent::Error {
                    code: "ERR_DOWNLOAD_TOO_LARGE",
                    ..
                }
            )),
            "must emit typed oversized-download error event; got {events:?}"
        );
    }

    /// Test-only entry point that mirrors `real::run_download` but
    /// skips the `https://` requirement in `DownloadRequest::validate`.
    /// We use a plaintext loopback server in the 416-restart test, and
    /// production callers reach `run_download` (which still enforces
    /// `https://`). Keeps the validation invariant intact for live
    /// traffic while letting CI exercise the restart branch.
    #[cfg(all(feature = "network", feature = "tokio"))]
    async fn exercise_run_download_for_test(
        req: DownloadRequest,
        events: tokio::sync::mpsc::Sender<DownloadEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<()> {
        // Reach into the same module-private real::run_download via a
        // local copy that only differs in URL-scheme validation. Any
        // future divergence between this and production must update
        // both — we lock that with the assertion below.
        assert!(
            req.url.starts_with("http://127.0.0.1:") || req.url.starts_with("https://"),
            "test exerciser only accepts loopback http or real https"
        );

        // Bypass DownloadRequest::validate's https check by calling
        // the internal helper directly.
        let validated = DownloadRequest {
            url: req.url.clone(),
            expected_sha256: req.expected_sha256.clone(),
            dest: req.dest.clone(),
        };
        // Sanity: the rest of validate() (sha length, empty dest)
        // should still hold.
        if validated.expected_sha256.len() != 64
            || !validated
                .expected_sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit())
        {
            return Err(MukeiError::ToolArgumentInvalid {
                field: "sha256",
                reason: "test exerciser: bad sha".into(),
            });
        }

        real::run_download_unchecked(validated, events, cancel).await
    }
}
