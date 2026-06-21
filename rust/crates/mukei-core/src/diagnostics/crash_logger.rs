//! `mukei_core::diagnostics::crash_logger` — TRD §36.1, PRD §4 FMEA.
//!
//! Persists *local-only* crash records so that the next session can
//! detect a regression loop. NO remote uploads.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[allow(unused_imports)]
use crate::diagnostics::logger;

/// Stable 256-bit fingerprint used to identify a regression. Same
/// location + same panic payload in two consecutive boots => CrashLoopDetected.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CrashFingerprint(String);

impl CrashFingerprint {
    pub fn as_str(&self) -> &str { &self.0 }

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
            location,
            reason,
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
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            append_lock: std::sync::Mutex::new(()),
        })
    }

    pub fn dir(&self) -> &Path { &self.dir }

    fn file_for(&self, fp: &CrashFingerprint) -> PathBuf {
        self.dir.join(format!("{fp}.json"))
    }

    pub fn append(&self, rec: &CrashRecord) {
        let _g = self.append_lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.file_for(&rec.fingerprint);
        let body = serde_json::to_vec_pretty(rec).unwrap_or_default();
        let _ = fs::write(&path, body);
    }

    /// Recent crashes for the given fingerprint. Used by the boot path
    /// (§36.1) to decide whether the user is stuck in a crash loop.
    pub fn recent_for(&self, fp: &CrashFingerprint) -> io::Result<Vec<CrashRecord>> {
        let path = self.file_for(fp);
        match fs::read(&path) {
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
            if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
            let bytes = match fs::read(&path) { Ok(b) => b, Err(_) => continue };
            let rec: CrashRecord = match serde_json::from_slice(&bytes) { Ok(r) => r, Err(_) => continue };
            if newest.as_ref().map(|(ts, _)| rec.ts > *ts).unwrap_or(true) {
                newest = Some((rec.ts, rec));
            }
        }
        Ok(newest.map(|(_, r)| r))
    }
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
}
