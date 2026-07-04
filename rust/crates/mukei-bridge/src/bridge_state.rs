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

/// RAII guard that removes its destination path from the in-flight
/// download registry on `Drop`.
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
