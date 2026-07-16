//! Generation-safe registry for process-scoped Android native runtimes.

use std::collections::HashMap;
use std::sync::Arc;

use jni::sys::jlong;
use mukei_core::application_runtime::MukeiRuntime;

// Child JNI modules intentionally access these crate-root helpers through this
// module. Keeping each transport in a real Rust module prevents textual
// `include!` import collisions while preserving the exported JNI symbol names.
use super::{
    guarded_bytes, invalid_handle_payload, runtime_entry, runtime_services, serialize,
    to_java_bytes, RUNTIMES,
};

const MAX_GENERATION: u32 = 0x7fff_ffff;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeHandle {
    slot: u32,
    generation: u32,
}

impl RuntimeHandle {
    fn new(slot: u32, generation: u32) -> Option<Self> {
        if slot == 0 || generation == 0 || generation > MAX_GENERATION {
            return None;
        }
        Some(Self { slot, generation })
    }

    pub(crate) fn encode(self) -> jlong {
        (((self.generation as u64) << 32) | self.slot as u64) as jlong
    }

    pub(crate) fn decode(raw: jlong) -> Option<Self> {
        if raw <= 0 {
            return None;
        }
        let raw = raw as u64;
        Self::new(raw as u32, (raw >> 32) as u32)
    }
}

struct RuntimeSlot {
    generation: u32,
    runtime: Arc<MukeiRuntime>,
}

#[derive(Default)]
pub(crate) struct RuntimeRegistry {
    next_slot: u32,
    free_slots: Vec<u32>,
    generations: HashMap<u32, u32>,
    entries: HashMap<u32, RuntimeSlot>,
}

impl RuntimeRegistry {
    pub(crate) fn insert(&mut self, runtime: Arc<MukeiRuntime>) -> Option<jlong> {
        let slot = match self.free_slots.pop() {
            Some(slot) => slot,
            None => {
                let next = self.next_slot.checked_add(1)?;
                if next == 0 {
                    return None;
                }
                self.next_slot = next;
                next
            }
        };
        let generation = self
            .generations
            .entry(slot)
            .and_modify(|value| {
                *value = if *value >= MAX_GENERATION { 1 } else { *value + 1 };
            })
            .or_insert(1);
        let handle = RuntimeHandle::new(slot, *generation)?;
        self.entries.insert(
            slot,
            RuntimeSlot {
                generation: *generation,
                runtime,
            },
        );
        Some(handle.encode())
    }

    pub(crate) fn get(&self, raw: jlong) -> Option<Arc<MukeiRuntime>> {
        let handle = RuntimeHandle::decode(raw)?;
        let slot = self.entries.get(&handle.slot)?;
        if slot.generation != handle.generation {
            return None;
        }
        Some(Arc::clone(&slot.runtime))
    }

    pub(crate) fn remove(&mut self, raw: jlong) -> Option<Arc<MukeiRuntime>> {
        let handle = RuntimeHandle::decode(raw)?;
        let slot = self.entries.get(&handle.slot)?;
        if slot.generation != handle.generation {
            return None;
        }
        let slot = self.entries.remove(&handle.slot)?;
        self.free_slots.push(handle.slot);
        Some(slot.runtime)
    }

    #[cfg(test)]
    fn active_count(&self) -> usize {
        self.entries.len()
    }
}

#[path = "secure_runtime_jni.rs"]
mod secure_runtime_jni;
#[path = "remote_tools_jni.rs"]
mod remote_tools_jni;

#[cfg(test)]
mod tests {
    use super::*;
    use mukei_core::application_runtime::RuntimeConfig;

    fn runtime() -> Arc<MukeiRuntime> {
        Arc::new(
            MukeiRuntime::create(RuntimeConfig {
                app_data_dir: "/tmp/mukei-jni-registry".into(),
                worker_threads: 1,
                max_blocking_threads: 1,
                event_capacity: 32,
            })
            .expect("runtime"),
        )
    }

    #[test]
    fn stale_generation_cannot_resolve_reused_slot() {
        let mut registry = RuntimeRegistry::default();
        let first = registry.insert(runtime()).expect("first handle");
        let removed = registry.remove(first).expect("first runtime");
        removed.shutdown();

        let second = registry.insert(runtime()).expect("second handle");
        assert_ne!(first, second);
        assert!(registry.get(first).is_none());
        assert!(registry.get(second).is_some());
    }

    #[test]
    fn duplicate_remove_is_rejected() {
        let mut registry = RuntimeRegistry::default();
        let handle = registry.insert(runtime()).expect("handle");
        assert!(registry.remove(handle).is_some());
        assert!(registry.remove(handle).is_none());
        assert_eq!(registry.active_count(), 0);
    }
}
