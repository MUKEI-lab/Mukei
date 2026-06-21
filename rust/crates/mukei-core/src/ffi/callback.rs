//! Cross-boundary callback metadata (TRD §1.3, §1.5).
//!
//! The bridge crate maps this to CXX-Qt signal tuples. Keeping the data
//! shape here lets us unit-test the round-trip without booting Qt.

use serde::{Deserialize, Serialize};

/// Identifies a single FFI boundary crossing.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FfiBoundaryId(pub u64);

impl FfiBoundaryId {
    pub const fn null() -> Self {
        Self(0)
    }
    pub fn next() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

/// Toggled state used by the QML signal hub when a long-running edge
/// passes its pinned generation counter.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FfiStateChange {
    pub boundary: FfiBoundaryId,
    pub old:      String,
    pub new:      String,
}

/// Registration record — QML registers each `unsafe extern "RustQt"`
/// slot through this struct rather than directly raw, so the bridge
/// crate has a single point at which to enforce the `catch_unwind`
/// wrapper.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FfiCallbackRegistration {
    pub boundary:     FfiBoundaryId,
    pub signal_name:  String,
    pub qml_actor:    String,
    /// true if `Qt::QueuedConnection` is mandated (TRD REQ-ARCH-03).
    pub queued_only:  bool,
}
