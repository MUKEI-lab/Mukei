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
//! - Resumes use HTTP `Range: bytes=<offset>-`. If the server returns
//!   `200 OK` instead of `206 Partial Content`, the `.partial` file is
//!   truncated and the download restarts from byte 0 — this matches the
//!   architect-review-blessed "rebuild on resume mismatch" path and is
//!   why [`crate::error::MukeiError::DownloadHashMismatch`] also covers
//!   "truncated resume hash mismatch".
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

/// Progress event emitted during a download. The bridge crate translates
/// these into QML's `download_progress(progress: f64, status: QString)`
/// signal. Keep this enum stable — the QML side switches on `status`.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadEvent {
    /// Download has started. `total_bytes` is `None` for servers that
    /// don't return `Content-Length`.
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
        req.validate()?;

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

        let resp = request
            .send()
            .await
            .map_err(|e| MukeiError::NetworkError(format!("GET {}: {e}", req.url)))?;

        let status = resp.status();
        if !status.is_success() {
            let msg = format!("HTTP {status} for {}", req.url);
            let _ = events
                .send(DownloadEvent::Error {
                    code: "ERR_NETWORK",
                    message: msg.clone(),
                })
                .await;
            return Err(MukeiError::NetworkError(msg));
        }

        // 200 OK instead of 206 means the server ignored Range — restart.
        if resume_from > 0 && status != reqwest::StatusCode::PARTIAL_CONTENT {
            tokio::fs::remove_file(&partial)
                .await
                .map_err(|e| MukeiError::Io(format!("rm partial for restart: {e}")))?;
            resume_from = 0;
        }

        let total_bytes = resp.content_length().map(|c| c + resume_from);

        let _ = events.send(DownloadEvent::Started { total_bytes }).await;

        // Open .partial in resume-or-create mode at `resume_from`. We
        // deliberately set `truncate(false)` so resuming preserves the
        // bytes streamed in a prior session; the explicit value also
        // silences clippy::suspicious_open_options under `-D warnings`
        // (CI gate).
        let mut out = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(false)
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

            if let Some(tot) = total_bytes {
                if tot > 0 {
                    let pct = downloaded as f64 / tot as f64;
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
}
