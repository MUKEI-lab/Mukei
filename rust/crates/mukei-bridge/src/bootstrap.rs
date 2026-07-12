//! Secure database bootstrap state and key lifecycle.
//!
//! The platform provider owns the non-exportable wrapping key. Rust only sees
//! the randomly generated database key long enough to pass it directly into
//! SQLCipher. The provider persists ciphertext/wrapped material; plaintext is
//! held in `Zeroizing` memory and is never serialized, logged, or sent to QML.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

use zeroize::Zeroizing;

#[cfg(target_os = "android")]
use crate::android_secret_store;

pub(crate) const DATABASE_KEY_ALIAS: &str = "mukei.database.key.v1";
pub(crate) const WRAPPING_KEY_PROBE_ALIAS: &str = "mukei.database.wrap_probe.v1";
const WRAPPING_KEY_PROBE: &[u8] = b"mukei-database-wrap-probe-v1";
const DATABASE_KEY_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum SecureBootstrapState {
    Uninitialized = 0,
    CreatingWrappingKey = 1,
    CreatingDatabaseKey = 2,
    WrappingDatabaseKey = 3,
    UnwrappingDatabaseKey = 4,
    OpeningDatabase = 5,
    Ready = 6,
    KeyInvalidated = 7,
    WrappedKeyCorrupt = 8,
    DatabaseOpenFailed = 9,
    ResetRequired = 10,
}

impl SecureBootstrapState {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::CreatingWrappingKey,
            2 => Self::CreatingDatabaseKey,
            3 => Self::WrappingDatabaseKey,
            4 => Self::UnwrappingDatabaseKey,
            5 => Self::OpeningDatabase,
            6 => Self::Ready,
            7 => Self::KeyInvalidated,
            8 => Self::WrappedKeyCorrupt,
            9 => Self::DatabaseOpenFailed,
            10 => Self::ResetRequired,
            _ => Self::Uninitialized,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BootstrapStart {
    Started(u64),
    AlreadyReady,
    InProgress(SecureBootstrapState),
    ResetRequired,
}

pub(crate) struct SecureBootstrapCoordinator {
    state: AtomicU8,
    generation: AtomicU64,
}

impl SecureBootstrapCoordinator {
    pub(crate) fn new() -> Self {
        Self {
            state: AtomicU8::new(SecureBootstrapState::Uninitialized as u8),
            generation: AtomicU64::new(0),
        }
    }

    pub(crate) fn state(&self) -> SecureBootstrapState {
        SecureBootstrapState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub(crate) fn begin(&self) -> BootstrapStart {
        loop {
            let current = self.state();
            match current {
                SecureBootstrapState::Ready => return BootstrapStart::AlreadyReady,
                SecureBootstrapState::CreatingWrappingKey
                | SecureBootstrapState::CreatingDatabaseKey
                | SecureBootstrapState::WrappingDatabaseKey
                | SecureBootstrapState::UnwrappingDatabaseKey
                | SecureBootstrapState::OpeningDatabase => {
                    return BootstrapStart::InProgress(current)
                }
                SecureBootstrapState::KeyInvalidated
                | SecureBootstrapState::WrappedKeyCorrupt
                | SecureBootstrapState::DatabaseOpenFailed
                | SecureBootstrapState::ResetRequired => return BootstrapStart::ResetRequired,
                SecureBootstrapState::Uninitialized => {}
            }
            if self
                .state
                .compare_exchange(
                    SecureBootstrapState::Uninitialized as u8,
                    SecureBootstrapState::CreatingWrappingKey as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
                return BootstrapStart::Started(generation);
            }
        }
    }

    pub(crate) fn transition(&self, state: SecureBootstrapState) {
        self.state.store(state as u8, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

pub(crate) trait SecureKeyProvider {
    fn exists(&self, alias: &str) -> Result<bool, String>;
    fn store(&self, alias: &str, plaintext: &[u8]) -> Result<(), String>;
    fn load(&self, alias: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String>;
}

pub(crate) struct PlatformSecureKeyProvider;

impl SecureKeyProvider for PlatformSecureKeyProvider {
    fn exists(&self, alias: &str) -> Result<bool, String> {
        #[cfg(target_os = "android")]
        {
            return android_secret_store::exists(alias);
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = alias;
            Err("secure database key storage is unavailable on this platform".to_string())
        }
    }

    fn store(&self, alias: &str, plaintext: &[u8]) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            return android_secret_store::store(alias, plaintext);
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (alias, plaintext);
            Err("secure database key storage is unavailable on this platform".to_string())
        }
    }

    fn load(&self, alias: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        #[cfg(target_os = "android")]
        {
            return android_secret_store::load(alias);
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = alias;
            Err("secure database key storage is unavailable on this platform".to_string())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SecureBootstrapFailure {
    PlatformUnavailable,
    KeyInvalidated,
    WrappedKeyCorrupt,
    RandomGenerationFailed,
}

impl SecureBootstrapFailure {
    pub(crate) fn state(&self) -> SecureBootstrapState {
        match self {
            Self::KeyInvalidated => SecureBootstrapState::KeyInvalidated,
            Self::WrappedKeyCorrupt => SecureBootstrapState::WrappedKeyCorrupt,
            Self::PlatformUnavailable | Self::RandomGenerationFailed => {
                SecureBootstrapState::ResetRequired
            }
        }
    }

    pub(crate) fn safe_code(&self) -> &'static str {
        match self {
            Self::PlatformUnavailable => "ERR_SAFE_STORAGE",
            Self::KeyInvalidated => "ERR_DB_KEY_INVALIDATED",
            Self::WrappedKeyCorrupt => "ERR_WRAPPED_KEY",
            Self::RandomGenerationFailed => "ERR_DB_KEY_GENERATION",
        }
    }

    pub(crate) fn safe_message(&self) -> &'static str {
        match self {
            Self::PlatformUnavailable => "Secure key storage is unavailable.",
            Self::KeyInvalidated => {
                "The device security key changed. Local encrypted storage must be recovered or reset."
            }
            Self::WrappedKeyCorrupt => {
                "The encrypted database key record is damaged and cannot be used safely."
            }
            Self::RandomGenerationFailed => "A secure database key could not be created.",
        }
    }
}

pub(crate) struct PreparedDatabaseKey {
    pub(crate) key: Zeroizing<Vec<u8>>,
    pub(crate) first_install: bool,
}

impl std::fmt::Debug for PreparedDatabaseKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedDatabaseKey")
            .field("key", &"[REDACTED]")
            .field("key_len", &self.key.len())
            .field("first_install", &self.first_install)
            .finish()
    }
}

#[cfg(test)]
pub(crate) fn prepare_database_key(
    coordinator: &SecureBootstrapCoordinator,
    provider: &impl SecureKeyProvider,
) -> Result<PreparedDatabaseKey, SecureBootstrapFailure> {
    prepare_database_key_with_observer(coordinator, provider, |_| {})
}

pub(crate) fn prepare_database_key_with_observer(
    coordinator: &SecureBootstrapCoordinator,
    provider: &impl SecureKeyProvider,
    mut observe: impl FnMut(SecureBootstrapState),
) -> Result<PreparedDatabaseKey, SecureBootstrapFailure> {
    let mut transition = |state| {
        coordinator.transition(state);
        observe(state);
    };
    let key_exists = provider
        .exists(DATABASE_KEY_ALIAS)
        .map_err(|_| SecureBootstrapFailure::PlatformUnavailable)?;

    if !key_exists {
        transition(SecureBootstrapState::CreatingWrappingKey);
        // Writing the non-secret probe forces the platform provider to create
        // its non-exportable wrapping key before any database key exists.
        if !provider
            .exists(WRAPPING_KEY_PROBE_ALIAS)
            .map_err(|_| SecureBootstrapFailure::PlatformUnavailable)?
        {
            provider
                .store(WRAPPING_KEY_PROBE_ALIAS, WRAPPING_KEY_PROBE)
                .map_err(|_| SecureBootstrapFailure::PlatformUnavailable)?;
        }

        transition(SecureBootstrapState::CreatingDatabaseKey);
        let mut key = Zeroizing::new(vec![0u8; DATABASE_KEY_BYTES]);
        getrandom::getrandom(key.as_mut_slice())
            .map_err(|_| SecureBootstrapFailure::RandomGenerationFailed)?;

        transition(SecureBootstrapState::WrappingDatabaseKey);
        provider
            .store(DATABASE_KEY_ALIAS, key.as_slice())
            .map_err(|_| SecureBootstrapFailure::PlatformUnavailable)?;
        return Ok(PreparedDatabaseKey {
            key,
            first_install: true,
        });
    }

    transition(SecureBootstrapState::UnwrappingDatabaseKey);
    match provider
        .load(DATABASE_KEY_ALIAS)
        .map_err(|_| SecureBootstrapFailure::PlatformUnavailable)?
    {
        Some(key) if key.len() == DATABASE_KEY_BYTES => Ok(PreparedDatabaseKey {
            key,
            first_install: false,
        }),
        Some(_) => Err(SecureBootstrapFailure::WrappedKeyCorrupt),
        None => {
            let probe_exists = provider
                .exists(WRAPPING_KEY_PROBE_ALIAS)
                .map_err(|_| SecureBootstrapFailure::PlatformUnavailable)?;
            if !probe_exists {
                return Err(SecureBootstrapFailure::WrappedKeyCorrupt);
            }
            match provider
                .load(WRAPPING_KEY_PROBE_ALIAS)
                .map_err(|_| SecureBootstrapFailure::PlatformUnavailable)?
            {
                Some(probe) if probe.as_slice() == WRAPPING_KEY_PROBE => {
                    Err(SecureBootstrapFailure::WrappedKeyCorrupt)
                }
                _ => Err(SecureBootstrapFailure::KeyInvalidated),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use parking_lot::Mutex;

    #[derive(Default)]
    struct FakeProvider {
        values: Mutex<HashMap<String, Vec<u8>>>,
        unreadable: Mutex<HashMap<String, bool>>,
    }

    impl FakeProvider {
        fn set_unreadable(&self, alias: &str) {
            self.unreadable.lock().insert(alias.to_string(), true);
        }
    }

    impl SecureKeyProvider for FakeProvider {
        fn exists(&self, alias: &str) -> Result<bool, String> {
            Ok(self.values.lock().contains_key(alias))
        }

        fn store(&self, alias: &str, plaintext: &[u8]) -> Result<(), String> {
            self.values
                .lock()
                .insert(alias.to_string(), plaintext.to_vec());
            Ok(())
        }

        fn load(&self, alias: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
            if self.unreadable.lock().get(alias).copied().unwrap_or(false) {
                return Ok(None);
            }
            Ok(self
                .values
                .lock()
                .get(alias)
                .cloned()
                .map(Zeroizing::new))
        }
    }

    #[test]
    fn sol03_first_install_secure_bootstrap_transitions_and_reuses_key_on_restart() {
        let provider = FakeProvider::default();
        let coordinator = SecureBootstrapCoordinator::new();
        assert!(matches!(coordinator.begin(), BootstrapStart::Started(1)));
        let mut first_states = Vec::new();
        let first = prepare_database_key_with_observer(&coordinator, &provider, |state| {
            first_states.push(state)
        })
        .unwrap();
        assert_eq!(
            first_states,
            vec![
                SecureBootstrapState::CreatingWrappingKey,
                SecureBootstrapState::CreatingDatabaseKey,
                SecureBootstrapState::WrappingDatabaseKey,
            ]
        );
        assert!(first.first_install);
        assert_eq!(first.key.len(), DATABASE_KEY_BYTES);
        let first_bytes = first.key.to_vec();
        coordinator.transition(SecureBootstrapState::OpeningDatabase);
        coordinator.transition(SecureBootstrapState::Ready);
        assert_eq!(coordinator.state(), SecureBootstrapState::Ready);

        let restart = SecureBootstrapCoordinator::new();
        assert!(matches!(restart.begin(), BootstrapStart::Started(1)));
        let mut restart_states = Vec::new();
        let second = prepare_database_key_with_observer(&restart, &provider, |state| {
            restart_states.push(state)
        })
        .unwrap();
        assert_eq!(restart_states, vec![SecureBootstrapState::UnwrappingDatabaseKey]);
        assert!(!second.first_install);
        assert_eq!(second.key.as_slice(), first_bytes.as_slice());
        assert_eq!(restart.state(), SecureBootstrapState::UnwrappingDatabaseKey);
    }

    #[test]
    fn sol03_corrupt_wrapped_database_key_is_explicit_failure() {
        let provider = FakeProvider::default();
        provider.store(WRAPPING_KEY_PROBE_ALIAS, WRAPPING_KEY_PROBE).unwrap();
        provider.store(DATABASE_KEY_ALIAS, &[1, 2, 3]).unwrap();
        let coordinator = SecureBootstrapCoordinator::new();
        let failure = prepare_database_key(&coordinator, &provider).unwrap_err();
        assert_eq!(failure, SecureBootstrapFailure::WrappedKeyCorrupt);
        assert_eq!(failure.state(), SecureBootstrapState::WrappedKeyCorrupt);
    }

    #[test]
    fn sol03_invalidated_wrapping_key_requires_reset_or_recovery() {
        let provider = FakeProvider::default();
        provider.store(WRAPPING_KEY_PROBE_ALIAS, WRAPPING_KEY_PROBE).unwrap();
        provider.store(DATABASE_KEY_ALIAS, &[7; DATABASE_KEY_BYTES]).unwrap();
        provider.set_unreadable(DATABASE_KEY_ALIAS);
        provider.set_unreadable(WRAPPING_KEY_PROBE_ALIAS);
        let coordinator = SecureBootstrapCoordinator::new();
        let failure = prepare_database_key(&coordinator, &provider).unwrap_err();
        assert_eq!(failure, SecureBootstrapFailure::KeyInvalidated);
        assert_eq!(failure.state(), SecureBootstrapState::KeyInvalidated);
    }

    #[test]
    fn sol03_concurrent_bootstrap_begin_has_one_authoritative_initializer() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let coordinator = Arc::new(SecureBootstrapCoordinator::new());
        let barrier = Arc::new(Barrier::new(8));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let coordinator = Arc::clone(&coordinator);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                coordinator.begin()
            }));
        }
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().expect("bootstrap thread should not panic"))
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, BootstrapStart::Started(_)))
                .count(),
            1
        );
        assert_eq!(coordinator.generation(), 1);
    }

    #[test]
    fn sol03_duplicate_ready_bootstrap_is_idempotent() {
        let coordinator = SecureBootstrapCoordinator::new();
        assert!(matches!(coordinator.begin(), BootstrapStart::Started(1)));
        coordinator.transition(SecureBootstrapState::Ready);
        assert_eq!(coordinator.begin(), BootstrapStart::AlreadyReady);
        assert_eq!(coordinator.generation(), 1);
    }

    #[test]
    fn sol03_plaintext_database_key_is_never_part_of_user_visible_state() {
        let provider = FakeProvider::default();
        let coordinator = SecureBootstrapCoordinator::new();
        let prepared = prepare_database_key(&coordinator, &provider).unwrap();
        let visible = serde_json::json!({
            "state": format!("{:?}", coordinator.state()),
            "generation": coordinator.generation(),
        })
        .to_string();
        let key_hex = prepared
            .key
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(!visible.contains(&key_hex));
    }
}
