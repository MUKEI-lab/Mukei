//! `mukei_core::ffi::tags` — TRD §1.2.5.
//!
//! Thinking-tag streaming detector. The QML side's "Thinking Accordion"
//! must open the moment the LLM emits the open tag, and close the
//! moment it emits the close tag.
//!
//! Algorithm (BUGFIX v0.7.4 #1: anywhere-in-window, no `ends_with`):
//!
//! - We carry a stable sliding window in memory: the last
//!   `TAG_WINDOW = 64` bytes of emitted text.
//! - Every time the agent core emits a new string fragment, we **search
//!   anywhere-in-the-window** for the open / close tag. BPE tokenisers
//!   commonly split the tag across multiple token boundaries, so
//!   `ends_with` is structurally wrong.
//! - When the open tag is found, the window is truncated up to (and
//!   including) the tag, the `opened` flag flips on, and we emit a
//!   `TagEvents::OPENED` flag to the caller. The close tag works the
//!   same way in reverse.
//!
//! BUGFIX v0.7.4 #2: every `truncate` is verified through `truncate_safe`
//! which `debug_assert!(buf.is_char_boundary(new_len))`. This means the
//! tag can be safely localised (e.g. to a Devanagari/Chinese sentinel)
//! without crashing the agent loop on non-ASCII char boundaries.

/// Sliding-window size. Big enough to span BPE splits, small enough to
/// keep this O(1) per chunk.
pub const TAG_WINDOW: usize = 64;

/// The thinking-block opening tag. Defined as `OPEN_OPEN ++ "think" ++ OPEN_CLOSE`
/// so we never embed the literal sequence inside raw Rust strings,
/// keeping the Rust tokeniser happy.
const OPEN_OPEN: &str = "<";
const OPEN_CLOSE: &str = ">";
const CLOSE_OPEN: &str = "</";

/// Lazily-built open / close tags. Const fn isn't quite enough for
/// `concat!` over &str slices in older toolchains so we build them at
/// the static level.
/// The literal opening tag (`<think>`) the LLM emits to enter a
/// chain-of-thought block. Built at runtime so the literal bytes never
/// appear in source.
pub fn open_tag() -> String {
    format!("{OPEN_OPEN}think{OPEN_CLOSE}")
}
/// The literal closing tag (`</think>`) the LLM emits to exit a
/// chain-of-thought block.
pub fn close_tag() -> String {
    format!("{CLOSE_OPEN}think{OPEN_CLOSE}")
}

/// Stream detector.
#[derive(Debug, Clone, Default)]
pub struct TagsStreaming {
    window: String,
    opened: bool,
}

impl TagsStreaming {
    /// Construct an empty detector with `opened = false` and an empty window.
    pub fn new() -> Self {
        Self::default()
    }

    /// Truncate `buf` to a valid char-boundary just *before* `at`.
    fn truncate_safe(buf: &mut String, at: usize) {
        debug_assert!(
            buf.is_char_boundary(at),
            "truncate_safe: position {at} is not a UTF-8 char boundary (buf len={})",
            buf.len()
        );
        buf.truncate(at);
    }

    /// Process a newly arrived chunk. Returns the events the QML side
    /// should be told about.
    pub fn push(&mut self, chunk: &str) -> TagEvents {
        let mut events = TagEvents::NONE;

        // Append + cap window. The cap must hold even if a single chunk
        // is larger than TAG_WINDOW: in that case we keep the trailing
        // TAG_WINDOW bytes (rounded down to a char boundary).
        self.window.push_str(chunk);
        if self.window.len() > TAG_WINDOW {
            let mut drop = self.window.len() - TAG_WINDOW;
            while drop < self.window.len() && !self.window.is_char_boundary(drop) {
                drop += 1;
            }
            self.window.drain(..drop);
        }

        let open = open_tag();
        let close = close_tag();

        // A single push may contain BOTH an open and a close (e.g. the
        // model emits a full "<think>...</think>" in one go). Iterate
        // until the window has no further state transitions to report.
        loop {
            let mut progressed = false;
            if self.opened {
                if let Some(pos) = self.window.find(close.as_str()) {
                    // Issue #8: the previous implementation called
                    // `self.window.clear()` here, which wiped ALL text
                    // that arrived after `</think>` in the same chunk.
                    // A model emitting `</think>Hello!` in one push
                    // would silently lose `Hello!`. We now keep the
                    // tail (`pos + close.len()` onwards) so a
                    // subsequent open within the same chunk is still
                    // detected AND the answer text following the close
                    // tag survives.
                    self.opened = false;
                    events |= TagEvents::CLOSED;
                    let close_end = pos + close.len();
                    let mut consumed = self.window[..close_end].to_string();
                    Self::truncate_safe(&mut consumed, close_end);
                    // Drop everything up to AND INCLUDING the close tag
                    // itself, but keep whatever followed it.
                    self.window.drain(..consumed.len());
                    progressed = true;
                }
            } else if let Some(pos) = self.window.find(open.as_str()) {
                let end = pos + open.len();
                let mut consumed = self.window[..end].to_string();
                Self::truncate_safe(&mut consumed, end);
                self.window.drain(..consumed.len());
                self.opened = true;
                events |= TagEvents::OPENED;
                progressed = true;
            }
            if !progressed {
                break;
            }
        }

        events
    }

    /// `true` if the detector last saw an open tag without a matching close.
    pub fn is_open(&self) -> bool {
        self.opened
    }

    /// Reset to the empty/closed state. Used at the start of every turn.
    pub fn reset(&mut self) {
        self.opened = false;
        self.window.clear();
    }
}

/// Bit-set of events that occurred during a single `push` call.
/// Hand-rolled to keep the core crate `bitflags`-free.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TagEvents(u8);

impl TagEvents {
    /// No state transition.
    pub const NONE: TagEvents = TagEvents(0);
    /// The opening tag appeared during this push.
    pub const OPENED: TagEvents = TagEvents(1 << 0);
    /// The closing tag appeared during this push.
    pub const CLOSED: TagEvents = TagEvents(1 << 1);

    /// `true` if `self` carries every bit in `other`.
    pub fn contains(self, other: TagEvents) -> bool {
        (self.0 & other.0) == other.0
    }
    /// `true` iff no transition was emitted by the last push.
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

    fn open() -> String {
        open_tag()
    }
    fn close() -> String {
        close_tag()
    }

    #[test]
    fn opens_on_seeing_open_tag() {
        let mut d = TagsStreaming::new();
        let chunk = format!("hello {} world", open());
        let ev = d.push(&chunk);
        assert!(ev.contains(TagEvents::OPENED));
        assert!(d.is_open());
    }

    #[test]
    fn closes_on_close_tag() {
        let mut d = TagsStreaming::new();
        let chunk1 = format!("hello {} body of thought", open());
        let chunk2 = format!("{} afterthought", close());
        d.push(&chunk1);
        let ev = d.push(&chunk2);
        assert!(ev.contains(TagEvents::CLOSED));
        assert!(!d.is_open());
    }

    #[test]
    fn close_tag_does_not_eat_trailing_text_in_same_chunk() {
        // Issue #8 regression: the previous `self.window.clear()` on the
        // close branch wiped any text after `</think>` in the same chunk.
        // A model emitting `</think>visible answer` in one push would
        // silently lose `visible answer`. The new code drains only up
        // to the close tag's end, preserving the tail in the window
        // for subsequent state transitions.
        let mut d = TagsStreaming::new();
        let combined = format!(
            "{}thinking aloud{} now a new open: {}",
            open(),
            close(),
            open()
        );
        let ev = d.push(&combined);
        // The same chunk should report ALL of: opened, closed, opened.
        assert!(ev.contains(TagEvents::OPENED));
        assert!(ev.contains(TagEvents::CLOSED));
        // We end up open again because the second `<think>` follows the
        // close in the same window.
        assert!(d.is_open());
    }

    #[test]
    fn spans_token_boundary_no_endswith() {
        // BPE tokenizers legitimately split the open tag across two
        // chunks; v0.7.4 detects anywhere-in-window so the open still fires.
        let mut d = TagsStreaming::new();
        let open_s = open();
        let mid = open_s.len() / 2;
        let (a, b) = open_s.split_at(mid);
        let _ = d.push(&format!("some preamble {a}"));
        assert!(!d.is_open());
        let _ = d.push(&format!("{b} continuation"));
        assert!(d.is_open());
    }

    #[test]
    fn close_then_open_in_same_session() {
        let mut d = TagsStreaming::new();
        let pattern = format!("{} thinking {} user message", open(), close());
        d.push(&pattern);
        assert!(!d.is_open());
        let again = format!("hello {} again", open());
        let ev = d.push(&again);
        assert!(ev.contains(TagEvents::OPENED));
        assert!(d.is_open());
    }

    #[test]
    fn window_is_bounded() {
        let mut d = TagsStreaming::new();
        let half = TAG_WINDOW / 2;
        let long = "x".repeat(TAG_WINDOW * 2);
        d.push(&long[..half]);
        d.push(&long[half..]);
        assert!(d.window.len() <= TAG_WINDOW);
    }

    #[test]
    fn reset_clears_state() {
        let mut d = TagsStreaming::new();
        let chunk = format!("hello {}", open());
        d.push(&chunk);
        assert!(d.is_open());
        d.reset();
        assert!(!d.is_open());
        assert!(d.window.is_empty());
    }
}
