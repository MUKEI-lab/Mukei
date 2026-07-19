//! Safe generation leases for values that cross a native transport boundary.
//!
//! The Android production path uses polling through a bounded event queue, not
//! framework callbacks or raw owner pointers. A lease is therefore a normal
//! reference-counted Rust value with generation and instance checks.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use thiserror::Error;

static NEXT_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

/// Lease validation failures.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    /// The lease was never active or has been permanently closed.
    #[error("boundary lease has been released")]
    Released,
    /// The owner was rebound after this lease was captured.
    #[error("boundary generation mismatch (expected {expected}, current {current})")]
    GenerationMismatch {
        /// Captured generation.
        expected: u64,
        /// Current generation.
        current: u64,
    },
    /// A different owner instance is being observed.
    #[error("boundary instance mismatch (expected {expected}, current {current})")]
    InstanceMismatch {
        /// Captured instance identity.
        expected: u64,
        /// Current instance identity.
        current: u64,
    },
    /// Work panicked while executed under a validated lease.
    #[error("boundary operation panicked")]
    Panic,
}

/// Shared owner state for a boundary lease.
#[derive(Debug)]
pub struct Inner {
    generation: AtomicU64,
    instance_id: u64,
    released: AtomicBool,
}

impl Inner {
    /// Allocate a fresh owner state.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            generation: AtomicU64::new(1),
            instance_id: NEXT_INSTANCE_ID.fetch_add(1, Ordering::Relaxed),
            released: AtomicBool::new(false),
        })
    }

    /// Advance the generation for a new consumer binding.
    pub fn bump(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Permanently close this owner state.
    pub fn tombstone(&self) {
        self.released.store(true, Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Current generation.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Process-unique instance identity.
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }
}

/// Safe, cloneable lease captured from one native boundary owner.
#[derive(Clone, Debug)]
pub struct BoundaryLease {
    owner: Option<Arc<Inner>>,
    generation: u64,
    instance_id: u64,
}

impl BoundaryLease {
    /// Capture a lease from an active owner.
    pub fn capture(owner: &Arc<Inner>) -> Self {
        Self {
            owner: Some(Arc::clone(owner)),
            generation: owner.generation(),
            instance_id: owner.instance_id(),
        }
    }

    /// Invalid sentinel used before a runtime owner exists.
    pub const fn invalid() -> Self {
        Self {
            owner: None,
            generation: 0,
            instance_id: 0,
        }
    }

    /// Whether this lease has an owner and currently validates.
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }

    /// Captured generation.
    pub const fn generation_snapshot(&self) -> u64 {
        self.generation
    }

    /// Captured instance identity.
    pub const fn instance_snapshot(&self) -> u64 {
        self.instance_id
    }

    /// Validate this lease against its current owner state.
    pub fn validate(&self) -> Result<(), GuardError> {
        let owner = self.owner.as_ref().ok_or(GuardError::Released)?;
        if owner.released.load(Ordering::Acquire) {
            return Err(GuardError::Released);
        }
        let current_instance = owner.instance_id();
        if current_instance != self.instance_id {
            return Err(GuardError::InstanceMismatch {
                expected: self.instance_id,
                current: current_instance,
            });
        }
        let current_generation = owner.generation();
        if current_generation != self.generation {
            return Err(GuardError::GenerationMismatch {
                expected: self.generation,
                current: current_generation,
            });
        }
        Ok(())
    }

    /// Execute work only while the lease is current and contain panics.
    pub fn try_call<T>(
        &self,
        operation: impl FnOnce() -> Result<T, GuardError>,
    ) -> Result<T, GuardError> {
        self.validate()?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
            .map_err(|_| GuardError::Panic)?
    }
}

/// Temporary source-compatible name for callers migrating to [`BoundaryLease`].
#[deprecated(note = "Use BoundaryLease; Android uses queue polling instead of owner callbacks")]
pub type CallbackGuard = BoundaryLease;

/// Execute a fallible operation under a safe boundary lease.
#[macro_export]
macro_rules! callback_with_guard {
    ($lease:expr, $operation:block) => {{
        $lease.try_call(|| $operation)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_generation_is_rejected() {
        let owner = Inner::new();
        let lease = BoundaryLease::capture(&owner);
        owner.bump();
        assert!(matches!(
            lease.validate(),
            Err(GuardError::GenerationMismatch { .. })
        ));
    }

    #[test]
    fn tombstoned_owner_is_rejected() {
        let owner = Inner::new();
        let lease = BoundaryLease::capture(&owner);
        owner.tombstone();
        assert_eq!(lease.validate(), Err(GuardError::Released));
    }
}
