//! Platform-neutral state and streaming types shared by native transports.
//!
//! This module contains no UI toolkit, Android framework, JNI, or desktop
//! assumptions. Kotlin/Compose and any future client consume these values
//! through a versioned transport adapter.

use serde::{Deserialize, Serialize};

/// Maximum retained text used while detecting streamed reasoning delimiters.
pub const TAG_WINDOW: usize = 64;

/// Opening delimiter emitted by supported model templates.
pub fn open_tag() -> String {
    "<think>".to_owned()
}

/// Closing delimiter emitted by supported model templates.
pub fn close_tag() -> String {
    "</think>".to_owned()
}

/// Stable snapshot of the native application runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeSnapshot {
    /// Runtime has been allocated but initialization has not completed.
    Uninitialized,
    /// Required model is absent or failed integrity validation.
    ModelMissing {
        /// Expected model digest.
        expected_sha256: String,
    },
    /// A model transfer is in progress.
    Downloading {
        /// Bytes durably written.
        bytes_so_far: u64,
        /// Expected total bytes.
        bytes_total: u64,
    },
    /// Native model resources are being prepared.
    Loading {
        /// Current loading stage.
        stage: LoadingStage,
    },
    /// Runtime is ready for commands.
    IdleReady {
        /// User-visible model alias.
        model_alias: String,
    },
    /// A generation operation is active.
    Inferring {
        /// Number of tokens emitted during the current turn.
        tokens_generated: u32,
    },
    /// A tool operation is active.
    ToolExecuting {
        /// Stable tool identifier.
        tool: String,
    },
    /// Persisted state is being recovered after process recreation.
    Recovering {
        /// Last durably persisted token index.
        last_token_index: u32,
    },
    /// Work is constrained by a platform thermal signal.
    ThermalThrottled {
        /// Platform-normalized thermal level.
        thermal_level: u8,
    },
    /// Runtime cannot continue safely.
    FatalError {
        /// Stable machine-readable error code.
        code: String,
    },
}

/// Native model loading stage.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadingStage {
    /// Reading model metadata.
    ReadingGguf,
    /// Allocating the inference cache.
    AllocatingKvCache,
    /// Loading tokenizer data.
    ParsingTokenizer,
    /// Running a warm-up pass.
    WarmingUp,
}

/// Generic state transition emitted by a native subsystem.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundaryStateChange {
    /// Stable subsystem or stream identity.
    pub stream_id: String,
    /// Previous stable state tag.
    pub previous: String,
    /// New stable state tag.
    pub current: String,
}

/// Bounded detector for reasoning-block delimiters split across token chunks.
#[derive(Debug, Clone, Default)]
pub struct StreamTagDetector {
    window: String,
    opened: bool,
}

impl StreamTagDetector {
    /// Create an empty detector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Process one streamed text fragment.
    pub fn push(&mut self, chunk: &str) -> TagEvents {
        let mut events = TagEvents::NONE;
        self.window.push_str(chunk);
        if self.window.len() > TAG_WINDOW {
            let mut drop_at = self.window.len() - TAG_WINDOW;
            while drop_at < self.window.len() && !self.window.is_char_boundary(drop_at) {
                drop_at += 1;
            }
            self.window.drain(..drop_at);
        }

        let open = open_tag();
        let close = close_tag();
        loop {
            let progressed = if self.opened {
                if let Some(position) = self.window.find(&close) {
                    let end = position + close.len();
                    self.window.drain(..end);
                    self.opened = false;
                    events |= TagEvents::CLOSED;
                    true
                } else {
                    false
                }
            } else if let Some(position) = self.window.find(&open) {
                let end = position + open.len();
                self.window.drain(..end);
                self.opened = true;
                events |= TagEvents::OPENED;
                true
            } else {
                false
            };

            if !progressed {
                break;
            }
        }
        events
    }

    /// Whether an opening delimiter has not yet been closed.
    pub fn is_open(&self) -> bool {
        self.opened
    }

    /// Reset the detector for a new operation.
    pub fn reset(&mut self) {
        self.window.clear();
        self.opened = false;
    }
}

/// Bit-set of delimiter transitions observed during one push.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TagEvents(u8);

impl TagEvents {
    /// No transition occurred.
    pub const NONE: Self = Self(0);
    /// An opening delimiter was observed.
    pub const OPENED: Self = Self(1 << 0);
    /// A closing delimiter was observed.
    pub const CLOSED: Self = Self(1 << 1);

    /// Whether all bits in `other` are present.
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Whether no transitions were observed.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOrAssign for TagEvents {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detector_handles_split_delimiter() {
        let mut detector = StreamTagDetector::new();
        detector.push("prefix <thi");
        let events = detector.push("nk>content");
        assert!(events.contains(TagEvents::OPENED));
        assert!(detector.is_open());
    }

    #[test]
    fn detector_preserves_following_transition() {
        let mut detector = StreamTagDetector::new();
        let events = detector.push("<think>x</think><think>");
        assert!(events.contains(TagEvents::OPENED));
        assert!(events.contains(TagEvents::CLOSED));
        assert!(detector.is_open());
    }
}
