use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex as ParkingMutex;
use tokio::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveDownload {
    pub(crate) model_id: Option<String>,
    pub(crate) destination: String,
}

pub(crate) fn single_active_download(
    downloads: &ParkingMutex<Vec<ActiveDownload>>,
) -> (Option<String>, Option<String>) {
    let downloads = downloads.lock();
    if downloads.len() == 1 {
        (
            downloads[0].model_id.clone(),
            Some(downloads[0].destination.clone()),
        )
    } else {
        (None, None)
    }
}

/// RAII guard that clears the chat re-entrancy flag on `Drop`.
pub(crate) struct BusyGuard(pub(crate) Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// RAII guard that removes its destination path from the in-flight download
/// registry on `Drop`. The async registry is never held while a QML callback
/// is emitted.
pub(crate) struct DownloadSlotGuard {
    pub(crate) registry: Arc<Mutex<HashSet<PathBuf>>>,
    pub(crate) dest: PathBuf,
}

impl Drop for DownloadSlotGuard {
    fn drop(&mut self) {
        let registry = self.registry.clone();
        let dest = std::mem::take(&mut self.dest);
        mukei_core::runtime::get().spawn(async move {
            registry.lock().await.remove(&dest);
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum RuntimePhase {
    Uninitialized = 0,
    Initializing = 1,
    DatabaseOpened = 2,
    AuditVerified = 3,
    Ready = 4,
    Quarantined = 5,
}

impl RuntimePhase {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Initializing,
            2 => Self::DatabaseOpened,
            3 => Self::AuditVerified,
            4 => Self::Ready,
            5 => Self::Quarantined,
            _ => Self::Uninitialized,
        }
    }
}

pub(crate) struct RuntimeCoordinator {
    phase: std::sync::atomic::AtomicU8,
    generation: std::sync::atomic::AtomicU64,
}

impl RuntimeCoordinator {
    pub(crate) fn new() -> Self {
        Self {
            phase: std::sync::atomic::AtomicU8::new(RuntimePhase::Uninitialized as u8),
            generation: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub(crate) fn phase(&self) -> RuntimePhase {
        RuntimePhase::from_u8(self.phase.load(Ordering::Acquire))
    }

    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn try_begin_initialization(&self) -> Result<u64, RuntimePhase> {
        loop {
            let current = self.phase();
            if current != RuntimePhase::Uninitialized {
                return Err(current);
            }
            if self
                .phase
                .compare_exchange(
                    current as u8,
                    RuntimePhase::Initializing as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return Ok(self.generation.fetch_add(1, Ordering::AcqRel) + 1);
            }
        }
    }

    pub(crate) fn transition(&self, phase: RuntimePhase) {
        self.phase.store(phase as u8, Ordering::Release);
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.phase() == RuntimePhase::Ready
    }
}

pub(crate) struct InitializationGuard {
    coordinator: Arc<RuntimeCoordinator>,
    committed: bool,
}

impl InitializationGuard {
    pub(crate) fn try_new(coordinator: Arc<RuntimeCoordinator>) -> Result<Self, RuntimePhase> {
        coordinator.try_begin_initialization()?;
        Ok(Self {
            coordinator,
            committed: false,
        })
    }

    #[cfg(feature = "rusqlite")]
    pub(crate) fn transition(&self, phase: RuntimePhase) {
        self.coordinator.transition(phase);
    }

    pub(crate) fn commit_ready(mut self) {
        self.coordinator.transition(RuntimePhase::Ready);
        self.committed = true;
    }
}

impl Drop for InitializationGuard {
    fn drop(&mut self) {
        if !self.committed {
            self.coordinator.transition(RuntimePhase::Quarantined);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sol03_failed_initialization_stays_quarantined_until_process_restart() {
        let coordinator = Arc::new(RuntimeCoordinator::new());
        let guard = InitializationGuard::try_new(coordinator.clone()).unwrap();
        drop(guard);
        assert_eq!(coordinator.phase(), RuntimePhase::Quarantined);
        assert!(matches!(
            InitializationGuard::try_new(coordinator),
            Err(RuntimePhase::Quarantined)
        ));
    }

    #[test]
    fn sol03_runtime_initialization_is_idempotently_guarded() {
        let coordinator = Arc::new(RuntimeCoordinator::new());
        let first = InitializationGuard::try_new(coordinator.clone()).unwrap();
        assert_eq!(coordinator.generation(), 1);
        assert!(matches!(
            InitializationGuard::try_new(coordinator.clone()),
            Err(RuntimePhase::Initializing)
        ));
        first.commit_ready();
        assert_eq!(coordinator.phase(), RuntimePhase::Ready);
        assert!(matches!(
            InitializationGuard::try_new(coordinator.clone()),
            Err(RuntimePhase::Ready)
        ));
        assert_eq!(coordinator.generation(), 1);
    }
}
