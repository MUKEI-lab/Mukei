//! Authoritative process-wide application runtime owner.
//!
//! CXX-Qt/JNI bootstrap requires one process-global handle, so a single
//! `OnceLock<ApplicationRuntime>` remains. Every mutable bridge service is a
//! field below that root; there are no independent database/download/secret
//! globals.
//!
//! Lock discipline:
//! - `parking_lot::Mutex` protects short, in-memory swaps/reads only and is
//!   never held across `.await`.
//! - `tokio::sync::Mutex` is reserved for state whose mutation spans async
//!   coordination (`downloads_in_flight`). Its guard is never held while
//!   emitting a QML callback.
//! - immutable service handles are shared as `Arc<T>`.
//! - no QML callback/event emission occurs while any mutable service lock is
//!   held.
//! - nested mutable locks are avoided. When two values are needed, clone each
//!   handle in a separate narrow critical section before awaiting work.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex as ParkingMutex;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use mukei_core::agent::AgentLoop;
use mukei_core::config::MukeiConfig;
use mukei_core::engine::ModelActivationService;
use mukei_core::tools::{RemoteFeaturePolicy, ToolRegistry};
use mukei_core::types::{BranchId, ConversationId};

use crate::async_bridge::AsyncRequestTracker;
use crate::bootstrap::SecureBootstrapCoordinator;
use crate::bridge_state::RuntimeCoordinator;
use crate::core_saf;
use crate::protocol::ProtocolRuntimeState;

pub(crate) struct ApplicationRuntime {
    lifecycle: LifecycleServices,
    persistence: PersistenceServices,
    agent: AgentServices,
    model_download: ModelDownloadServices,
    documents: DocumentServices,
    secrets: SecretServices,
    protocol: ProtocolServices,
}

struct LifecycleServices {
    coordinator: Arc<RuntimeCoordinator>,
    secure_bootstrap: Arc<SecureBootstrapCoordinator>,
    thermal_status: AtomicI32,
    requests: AsyncRequestTracker,
}

struct PersistenceServices {
    #[cfg(feature = "rusqlite")]
    database_pool: ParkingMutex<Option<Arc<mukei_core::storage::DatabasePool>>>,
    #[cfg(feature = "rusqlite")]
    audit_log_writer: Arc<mukei_core::storage::AuditLogWriter>,
}

struct AgentServices {
    agent_loop: ParkingMutex<Option<Arc<AgentLoop>>>,
    activation_service: Arc<ModelActivationService>,
    chat_session: ParkingMutex<Option<(ConversationId, BranchId)>>,
    config: ParkingMutex<Option<MukeiConfig>>,
    tool_registry: ParkingMutex<Arc<ToolRegistry>>,
}

struct ModelDownloadServices {
    downloads_in_flight: Arc<Mutex<HashSet<PathBuf>>>,
    model_base_dir: ParkingMutex<PathBuf>,
    model_dir: ParkingMutex<PathBuf>,
}

struct DocumentServices {
    saf_registry: Arc<core_saf::SafRegistry>,
}

struct ProtocolServices {
    runtime: ParkingMutex<ProtocolRuntimeState>,
}

struct SecretServices {
    brave_api_key: ParkingMutex<Option<Zeroizing<String>>>,
    tavily_api_key: ParkingMutex<Option<Zeroizing<String>>>,
    remote_feature_policy: ParkingMutex<RemoteFeaturePolicy>,
}

impl ApplicationRuntime {
    fn new() -> Self {
        Self {
            lifecycle: LifecycleServices {
                coordinator: Arc::new(RuntimeCoordinator::new()),
                secure_bootstrap: Arc::new(SecureBootstrapCoordinator::new()),
                thermal_status: AtomicI32::new(0),
                requests: AsyncRequestTracker::default(),
            },
            persistence: PersistenceServices {
                #[cfg(feature = "rusqlite")]
                database_pool: ParkingMutex::new(None),
                #[cfg(feature = "rusqlite")]
                audit_log_writer: Arc::new(mukei_core::storage::AuditLogWriter::new()),
            },
            agent: AgentServices {
                agent_loop: ParkingMutex::new(None),
                activation_service: ModelActivationService::new(false),
                chat_session: ParkingMutex::new(None),
                config: ParkingMutex::new(None),
                tool_registry: ParkingMutex::new(Arc::new(
                    ToolRegistry::with_web_search_keys_and_policy(
                        "missing-brave-key",
                        "missing-tavily-key",
                        RemoteFeaturePolicy::default(),
                    ),
                )),
            },
            model_download: ModelDownloadServices {
                downloads_in_flight: Arc::new(Mutex::new(HashSet::new())),
                model_base_dir: ParkingMutex::new(crate::default_model_base_dir()),
                model_dir: ParkingMutex::new(crate::default_model_base_dir().join("models")),
            },
            documents: DocumentServices {
                saf_registry: Arc::new(core_saf::SafRegistry::new()),
            },
            protocol: ProtocolServices {
                runtime: ParkingMutex::new(ProtocolRuntimeState::new()),
            },
            secrets: SecretServices {
                brave_api_key: ParkingMutex::new(None),
                tavily_api_key: ParkingMutex::new(None),
                remote_feature_policy: ParkingMutex::new(RemoteFeaturePolicy::default()),
            },
        }
    }

    pub(crate) fn runtime_coordinator(&self) -> &Arc<RuntimeCoordinator> {
        &self.lifecycle.coordinator
    }

    pub(crate) fn secure_bootstrap(&self) -> &Arc<SecureBootstrapCoordinator> {
        &self.lifecycle.secure_bootstrap
    }

    pub(crate) fn request_tracker(&self) -> &AsyncRequestTracker {
        &self.lifecycle.requests
    }

    pub(crate) fn thermal_status(&self) -> i32 {
        self.lifecycle.thermal_status.load(Ordering::Acquire)
    }

    pub(crate) fn set_thermal_status(&self, status: i32) {
        self.lifecycle
            .thermal_status
            .store(status, Ordering::Release);
    }

    #[cfg(feature = "rusqlite")]
    pub(crate) fn database_pool(&self) -> Option<Arc<mukei_core::storage::DatabasePool>> {
        self.persistence.database_pool.lock().clone()
    }

    #[cfg(feature = "rusqlite")]
    pub(crate) fn set_database_pool(&self, pool: Arc<mukei_core::storage::DatabasePool>) {
        *self.persistence.database_pool.lock() = Some(pool);
    }

    #[cfg(feature = "rusqlite")]
    pub(crate) fn audit_log_writer(&self) -> &Arc<mukei_core::storage::AuditLogWriter> {
        &self.persistence.audit_log_writer
    }

    pub(crate) fn agent_loop(&self) -> Option<Arc<AgentLoop>> {
        self.agent.agent_loop.lock().clone()
    }

    pub(crate) fn set_agent_loop(&self, agent_loop: Arc<AgentLoop>) {
        *self.agent.agent_loop.lock() = Some(agent_loop);
    }

    pub(crate) fn model_activation_service(&self) -> Arc<ModelActivationService> {
        self.agent.activation_service.clone()
    }

    pub(crate) fn chat_session(&self) -> Option<(ConversationId, BranchId)> {
        self.agent.chat_session.lock().clone()
    }

    pub(crate) fn set_chat_session(&self, session: Option<(ConversationId, BranchId)>) {
        *self.agent.chat_session.lock() = session;
    }

    pub(crate) fn config(&self) -> Option<MukeiConfig> {
        self.agent.config.lock().clone()
    }

    pub(crate) fn set_config(&self, config: MukeiConfig) {
        *self.agent.config.lock() = Some(config);
    }

    pub(crate) fn tool_registry(&self) -> Arc<ToolRegistry> {
        self.agent.tool_registry.lock().clone()
    }

    pub(crate) fn set_tool_registry(&self, registry: Arc<ToolRegistry>) {
        *self.agent.tool_registry.lock() = registry;
    }

    pub(crate) fn downloads_in_flight(&self) -> &Arc<Mutex<HashSet<PathBuf>>> {
        &self.model_download.downloads_in_flight
    }

    pub(crate) fn model_base_dir(&self) -> PathBuf {
        self.model_download.model_base_dir.lock().clone()
    }

    pub(crate) fn set_model_base_dir(&self, path: PathBuf) {
        *self.model_download.model_base_dir.lock() = path;
    }

    pub(crate) fn model_dir(&self) -> PathBuf {
        self.model_download.model_dir.lock().clone()
    }

    pub(crate) fn set_model_dir(&self, path: PathBuf) {
        *self.model_download.model_dir.lock() = path;
    }

    pub(crate) fn saf_registry(&self) -> &Arc<core_saf::SafRegistry> {
        &self.documents.saf_registry
    }

    pub(crate) fn brave_api_key(&self) -> &ParkingMutex<Option<Zeroizing<String>>> {
        &self.secrets.brave_api_key
    }

    pub(crate) fn tavily_api_key(&self) -> &ParkingMutex<Option<Zeroizing<String>>> {
        &self.secrets.tavily_api_key
    }

    pub(crate) fn remote_feature_policy(&self) -> RemoteFeaturePolicy {
        self.secrets.remote_feature_policy.lock().clone()
    }

    pub(crate) fn set_remote_feature_policy(&self, policy: RemoteFeaturePolicy) {
        *self.secrets.remote_feature_policy.lock() = policy;
    }

    pub(crate) fn protocol_state(&self) -> &ParkingMutex<ProtocolRuntimeState> {
        &self.protocol.runtime
    }
}

static APPLICATION_RUNTIME: OnceLock<ApplicationRuntime> = OnceLock::new();

pub(crate) fn application_runtime() -> &'static ApplicationRuntime {
    APPLICATION_RUNTIME.get_or_init(ApplicationRuntime::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sol03_root_runtime_is_single_authoritative_service_owner() {
        let first = application_runtime() as *const ApplicationRuntime;
        let second = application_runtime() as *const ApplicationRuntime;
        assert_eq!(first, second);
        assert!(Arc::ptr_eq(
            application_runtime().runtime_coordinator(),
            application_runtime().runtime_coordinator()
        ));
        let first_activation = application_runtime().model_activation_service();
        let second_activation = application_runtime().model_activation_service();
        assert!(Arc::ptr_eq(&first_activation, &second_activation));
    }

    #[test]
    fn sol03_duplicate_runtime_access_does_not_create_duplicate_services() {
        let first = application_runtime().downloads_in_flight().clone();
        let second = application_runtime().downloads_in_flight().clone();
        assert!(Arc::ptr_eq(&first, &second));
        let first_saf = application_runtime().saf_registry().clone();
        let second_saf = application_runtime().saf_registry().clone();
        assert!(Arc::ptr_eq(&first_saf, &second_saf));
    }
}
