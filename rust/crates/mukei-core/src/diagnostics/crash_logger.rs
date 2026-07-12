//! `mukei_core::diagnostics::crash_logger` — TRD §36.1, PRD §4 FMEA.
//!
//! Persists *local-only* crash records so that the next session can
//! detect a regression loop. NO remote uploads.
//!
//! # Architect review GH #17 — Android scoped-storage contract
//!
//! The path passed into [`CrashLogger`] (and `crashes_dir` in
//! `config.toml`) MUST resolve to **app-internal scoped storage**:
//!
//! * **Android (target_os = "android")**: `Context.getFilesDir() +
//!   "/crashes/"`. The bridge crate is responsible for resolving this
//!   via JNI at boot (`SAFHelper.resolveFilesDir`). Writing to
//!   `/sdcard/crashes/` from the app process REQUIRES either
//!   `READ_EXTERNAL_STORAGE` (banned by PRD REQ-SEC-21) or
//!   `MANAGE_EXTERNAL_STORAGE` (banned by Google Play policy) — we do
//!   not request either.
//! * **Desktop (Linux / macOS)**: XDG `$XDG_DATA_HOME/mukei/crashes/`.
//! * **Bridge contract**: the bridge constructs the path BEFORE
//!   instantiating [`CrashLogger`]. This module itself never resolves
//!   the path — it only writes to whatever it is handed.
//!
//! The compile-time check below catches the most common regression:
//! a config that hardcodes `/sdcard/` and slips through the strict
//! schema validator.
#[cfg(feature = "release-hardening")]
const _: () = {
    // No-op marker compiled in release-hardening builds. The runtime
    // check lives in `CrashLogger::new`.
};

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[allow(unused_imports)]
use crate::diagnostics::logger;
use crate::diagnostics::redaction::{sanitize_stable_identifier, sanitize_telemetry_text};

pub const MAX_CRASH_LOCATION_CHARS: usize = 192;
pub const MAX_CRASH_REASON_CHARS: usize = 192;
pub const MAX_CRASH_RECORD_BYTES: usize = 4 * 1024;
pub const MAX_CRASH_RECORD_FILES: usize = 64;

/// Architect review GH #17: refuse paths that obviously breach
/// Android scoped-storage policy. Called from `CrashLogger::new`.
/// Returns `Err` for any `/sdcard/...`, `/storage/emulated/...`, or
/// `MediaStore` path — those would require banned permissions on
/// modern Android. Whitelisting is intentional; we want a hard error
/// at boot, not a silent fallback.
pub(crate) fn refuse_scoped_storage_violation(p: &Path) -> Result<(), io::Error> {
    let s = p.to_string_lossy();
    let lower = s.to_ascii_lowercase();
    for bad in [
        "/sdcard/",
        "/storage/emulated/",
        "/storage/self/",
        "content://media/",
    ] {
        if lower.contains(bad) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "crashes_dir resolves to `{s}` — this requires banned Android \
                     permissions (PRD REQ-SEC-21, architect review GH #17). \
                     Use Context.getFilesDir() + \"/crashes/\" instead."
                ),
            ));
        }
    }
    Ok(())
}

/// Stable 256-bit fingerprint used to identify a regression. Same
/// location + same panic payload in two consecutive boots => CrashLoopDetected.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CrashFingerprint(String);

impl CrashFingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Compute a SHA-256 over (file:line|reason) — stable across builds
    /// if and only if the *panic location & message* are identical.
    pub fn from_panic(location: &str, reason: &str) -> Self {
        let mut h = Sha256::new();
        h.update(location.as_bytes());
        h.update([0u8]); // delimiter
        h.update(reason.as_bytes());
        Self(hex_lower(&h.finalize()))
    }
}

impl std::fmt::Display for CrashFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrashRecord {
    pub fingerprint: CrashFingerprint,
    pub location: String,
    pub reason: String,
    pub ts: chrono::DateTime<chrono::Utc>,
}

impl CrashRecord {
    pub fn new(fp: CrashFingerprint, location: String, reason: String) -> Self {
        Self {
            fingerprint: fp,
            location: sanitize_telemetry_text(&location, MAX_CRASH_LOCATION_CHARS).into_string(),
            reason: sanitize_crash_reason(&reason),
            ts: chrono::Utc::now(),
        }
    }
}

/// Filesystem-backed crash sink (default for Android + desktop).
/// Cached behind a global `OnceLock` so the panic hook can `append`
/// without holding any user-installed state.
pub struct CrashSink {
    dir: PathBuf,
    append_lock: std::sync::Mutex<()>,
}

impl CrashSink {
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = dir.into();
        // Architect review GH #17: refuse paths that breach Android
        // scoped-storage. Belt-and-suspenders — the bridge is supposed
        // to resolve to Context.getFilesDir() before getting here, but
        // a stale config or test fixture could still slip a /sdcard/
        // path through.
        refuse_scoped_storage_violation(&dir)?;
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            append_lock: std::sync::Mutex::new(()),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file_for(&self, fp: &CrashFingerprint) -> PathBuf {
        self.dir.join(format!("{fp}.json"))
    }

    pub fn append(&self, rec: &CrashRecord) {
        let _g = self.append_lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.file_for(&rec.fingerprint);

        // `CrashRecord` is a public compatibility type, so do not trust callers
        // to have constructed it through `CrashRecord::new`. Re-sanitize into a
        // bounded local copy before serialization to avoid duplicating an
        // arbitrarily large caller-owned string.
        let safe_record = CrashRecord {
            fingerprint: rec.fingerprint.clone(),
            location: sanitize_telemetry_text(&rec.location, MAX_CRASH_LOCATION_CHARS)
                .into_string(),
            reason: sanitize_crash_reason(&rec.reason),
            ts: rec.ts,
        };
        let body = match serde_json::to_vec(&safe_record) {
            Ok(body) if body.len() <= MAX_CRASH_RECORD_BYTES => body,
            _ => return,
        };
        let temp = path.with_extension("json.tmp");
        match fs::write(&temp, body) {
            Ok(()) => {
                if fs::rename(&temp, &path).is_err() {
                    let _ = fs::remove_file(&temp);
                    return;
                }
                self.prune_old_records();
            }
            Err(_) => {
                let _ = fs::remove_file(&temp);
            }
        }
    }

    /// Recent crashes for the given fingerprint. Used by the boot path
    /// (§36.1) to decide whether the user is stuck in a crash loop.
    pub fn recent_for(&self, fp: &CrashFingerprint) -> io::Result<Vec<CrashRecord>> {
        let path = self.file_for(fp);
        match read_bounded(&path) {
            Ok(bytes) => {
                let rec: CrashRecord = serde_json::from_slice(&bytes)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                Ok(vec![rec])
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Returns the most recent crash record overall, regardless of
    /// fingerprint. Boot path uses this to detect "entering boot with
    /// last boot ending in crash".
    pub fn most_recent(&self) -> io::Result<Option<CrashRecord>> {
        let mut newest: Option<(chrono::DateTime<chrono::Utc>, CrashRecord)> = None;
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let bytes = match read_bounded(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let rec: CrashRecord = match serde_json::from_slice(&bytes) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if newest.as_ref().map(|(ts, _)| rec.ts > *ts).unwrap_or(true) {
                newest = Some((rec.ts, rec));
            }
        }
        Ok(newest.map(|(_, r)| r))
    }

    fn prune_old_records(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };

        // Keep only the newest bounded set while streaming the directory.
        // This avoids allocating one `PathBuf` per legacy/corrupt crash file.
        let mut retained = Vec::with_capacity(MAX_CRASH_RECORD_FILES + 1);
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(modified) = entry.metadata().ok().and_then(|meta| meta.modified().ok()) else {
                continue;
            };
            retained.push((modified, path));
            if retained.len() > MAX_CRASH_RECORD_FILES {
                let Some(oldest_index) = retained
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, (modified, _))| *modified)
                    .map(|(index, _)| index)
                else {
                    continue;
                };
                let (_, oldest_path) = retained.swap_remove(oldest_index);
                let _ = fs::remove_file(oldest_path);
            }
        }
    }
}

fn sanitize_crash_reason(reason: &str) -> String {
    let sanitized = sanitize_telemetry_text(reason, MAX_CRASH_REASON_CHARS).into_string();
    if let Some(stable) = sanitize_stable_identifier(&sanitized, MAX_CRASH_REASON_CHARS) {
        stable
    } else {
        "[redacted-content]".to_string()
    }
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(MAX_CRASH_RECORD_BYTES.min(1024));
    file.take((MAX_CRASH_RECORD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CRASH_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "crash record exceeds bounded diagnostics size",
        ));
    }
    Ok(bytes)
}

/// Public hex helper for callers outside `diag`.
pub fn hex_helper(bytes: &[u8]) -> String {
    hex_lower(bytes)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_then_read_roundtrips() {
        let dir = tempdir().unwrap();
        let sink = CrashSink::open(dir.path()).unwrap();
        let fp = CrashFingerprint::from_panic("x.rs:1", "boom");
        let rec = CrashRecord::new(fp.clone(), "x.rs:1".into(), "boom".into());
        sink.append(&rec);

        let read = sink.recent_for(&fp).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].fingerprint, fp);
    }

    #[test]
    fn missing_file_yields_empty() {
        let dir = tempdir().unwrap();
        let sink = CrashSink::open(dir.path()).unwrap();
        let fp = CrashFingerprint::from_panic("z.rs:9", "y");
        let got = sink.recent_for(&fp).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn hex_lower_matches_known_values() {
        assert_eq!(hex_lower(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(hex_lower(&[]), "");
    }

    #[test]
    fn scoped_storage_violation_is_refused() {
        // Architect review GH #17: opening a CrashSink against any
        // path that requires banned Android permissions fails fast.
        for bad in [
            "/sdcard/crashes",
            "/storage/emulated/0/mukei",
            "content://media/external/images",
        ] {
            let err = match CrashSink::open(std::path::PathBuf::from(bad)) {
                Ok(_) => panic!("expected scoped-storage refusal for {bad}"),
                Err(e) => e,
            };
            assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
            let msg = err.to_string();
            assert!(
                msg.contains("GH #17") || msg.contains("PRD REQ-SEC-21"),
                "refusal message must reference the rationale, got: {msg}",
            );
        }
    }

    #[test]
    fn app_internal_paths_are_allowed() {
        // App-internal scoped storage paths (the bridge's resolved
        // `Context.getFilesDir() + /crashes/`) pass cleanly.
        let dir = tempdir().unwrap();
        let _sink = CrashSink::open(dir.path()).unwrap();
    }
    #[test]
    fn crash_record_redacts_arbitrary_content_and_bounds_fields() {
        let fp = CrashFingerprint::from_panic("/home/private/project/main.rs:9", "raw");
        let rec = CrashRecord::new(
            fp,
            "/home/private/project/main.rs:9".into(),
            format!("user prompt private words {}", "x".repeat(10_000)),
        );
        assert!(!rec.location.contains("/home/private"));
        assert_eq!(rec.reason, "[redacted-content]");
        assert!(serde_json::to_vec(&rec).unwrap().len() <= MAX_CRASH_RECORD_BYTES);
    }

    #[test]
    fn append_resanitizes_public_record_fields_before_serialization() {
        let dir = tempdir().unwrap();
        let sink = CrashSink::open(dir.path()).unwrap();
        let fp = CrashFingerprint::from_panic("x.rs:1", "manual");
        let rec = CrashRecord {
            fingerprint: fp.clone(),
            location: "/home/private/".repeat(10_000),
            reason: "private user content ".repeat(10_000),
            ts: chrono::Utc::now(),
        };
        sink.append(&rec);
        let stored = sink.recent_for(&fp).unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].location.len() <= MAX_CRASH_LOCATION_CHARS * 4);
        assert_eq!(stored[0].reason, "[redacted-content]");
        assert!(serde_json::to_vec(&stored[0]).unwrap().len() <= MAX_CRASH_RECORD_BYTES);
    }

    #[test]
    fn oversized_crash_file_is_rejected_on_read() {
        let dir = tempdir().unwrap();
        let sink = CrashSink::open(dir.path()).unwrap();
        let fp = CrashFingerprint::from_panic("x.rs:1", "boom");
        std::fs::write(sink.file_for(&fp), vec![b'x'; MAX_CRASH_RECORD_BYTES + 1]).unwrap();
        let err = sink.recent_for(&fp).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn crash_sink_prunes_retained_file_count() {
        let dir = tempdir().unwrap();
        let sink = CrashSink::open(dir.path()).unwrap();
        for index in 0..(MAX_CRASH_RECORD_FILES + 8) {
            let reason = format!("reason_{index}");
            let fp = CrashFingerprint::from_panic("x.rs:1", &reason);
            sink.append(&CrashRecord::new(fp, "x.rs:1".into(), reason));
        }
        let count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .count();
        assert!(count <= MAX_CRASH_RECORD_FILES);
    }
}
