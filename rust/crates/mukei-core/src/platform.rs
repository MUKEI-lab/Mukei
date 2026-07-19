//! Pull-based platform request broker used by native Android transports.
//!
//! Rust owns domain policy and operation state. Android-only services such as
//! `ContentResolver`, Android Keystore, thermal status and connectivity are
//! requested through this bounded broker and completed through JNI.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Maximum requests retained before the runtime applies backpressure.
pub const MAX_PLATFORM_REQUESTS: usize = 128;
/// Maximum requests returned by one JNI drain.
pub const MAX_PLATFORM_DRAIN_ITEMS: usize = 32;

/// One Android-only service request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlatformRequestKind {
    /// Stage a user-selected `content://` document into app-private storage.
    DocumentStage {
        /// Opaque source URI supplied by Kotlin.
        target: String,
        /// User-visible label.
        label: String,
        /// MIME type reported by the picker.
        mime_type: String,
    },
    /// Delete a previously staged app-private document.
    DocumentDelete {
        /// Canonical app-private path previously returned by Android.
        staged_path: String,
    },
    /// Wrap secret bytes using a non-exportable Android Keystore key.
    SecureKeyWrap {
        /// Stable Keystore alias.
        alias: String,
        /// Base64 plaintext bytes. Kotlin must zero temporary buffers promptly.
        plaintext_base64: String,
    },
    /// Unwrap secret bytes using a non-exportable Android Keystore key.
    SecureKeyUnwrap {
        /// Stable Keystore alias.
        alias: String,
        /// Base64 wrapper envelope produced by `secure_key_wrap`.
        wrapped_base64: String,
    },
    /// Read normalized Android thermal status.
    ThermalStatus,
    /// Read normalized Android connectivity state.
    NetworkStatus,
    /// Collect a content-free Android diagnostics snapshot.
    DiagnosticsSnapshot,
}

/// Request queued for the Kotlin platform processor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlatformRequest {
    /// Globally unique request identity.
    pub request_id: String,
    /// Associated Protocol V2 operation identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Android service operation.
    pub request: PlatformRequestKind,
}

/// Platform response status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformResponseStatus {
    /// Android completed the request.
    Succeeded,
    /// Android rejected or failed the request.
    Failed,
}

/// Kotlin response submitted back through JNI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlatformResponse {
    /// Request identity being completed.
    pub request_id: String,
    /// Terminal status.
    pub status: PlatformResponseStatus,
    /// Structured success payload.
    #[serde(default)]
    pub payload: Value,
    /// Stable failure code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Redacted failure message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// Bounded platform-request drain response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlatformRequestBatch {
    /// Requests moved to the Android processor.
    pub requests: Vec<PlatformRequest>,
    /// Whether additional requests remain queued.
    pub has_more: bool,
}

/// Content-free broker snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformBrokerSnapshot {
    /// Requests not yet drained by Kotlin.
    pub queued: usize,
    /// Requests drained but not yet completed.
    pub in_flight: usize,
    /// Completed responses waiting for Rust consumers.
    pub completed: usize,
}

/// Platform broker failures.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PlatformPortError {
    /// Request queue reached its defensive capacity.
    #[error("platform request queue is full")]
    QueueFull,
    /// Kotlin submitted a response for an unknown or already-completed request.
    #[error("unknown platform request")]
    UnknownRequest,
    /// Android reported a terminal failure.
    #[error("platform request failed: {0}")]
    Failed(String),
    /// Operation was cancelled while waiting for Android.
    #[error("platform request cancelled")]
    Cancelled,
    /// Android did not answer within the operation budget.
    #[error("platform request timed out")]
    Timeout,
}

#[derive(Default)]
struct BrokerState {
    pending: VecDeque<PlatformRequest>,
    in_flight: HashSet<String>,
    responses: HashMap<String, PlatformResponse>,
}

/// Thread-safe pull broker shared by the Rust runtime and JNI adapter.
pub struct PlatformRequestBroker {
    capacity: usize,
    state: Mutex<BrokerState>,
    pending_changed: Condvar,
    response_changed: tokio::sync::Notify,
}

impl Default for PlatformRequestBroker {
    fn default() -> Self {
        Self::new(MAX_PLATFORM_REQUESTS)
    }
}

impl PlatformRequestBroker {
    /// Create a broker with a bounded total outstanding-request capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, MAX_PLATFORM_REQUESTS),
            state: Mutex::new(BrokerState::default()),
            pending_changed: Condvar::new(),
            response_changed: tokio::sync::Notify::new(),
        }
    }

    /// Queue one Android service request and return its stable identity.
    pub fn enqueue(
        &self,
        operation_id: Option<String>,
        request: PlatformRequestKind,
    ) -> Result<String, PlatformPortError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let outstanding = state.pending.len() + state.in_flight.len() + state.responses.len();
        if outstanding >= self.capacity {
            return Err(PlatformPortError::QueueFull);
        }
        let request_id = Uuid::new_v4().to_string();
        state.pending.push_back(PlatformRequest {
            request_id: request_id.clone(),
            operation_id,
            created_at: Utc::now(),
            request,
        });
        self.pending_changed.notify_all();
        Ok(request_id)
    }

    /// Drain requests for Kotlin. The wait is synchronous because JNI owns the
    /// calling thread; domain executors never call this method.
    pub fn drain(&self, limit: usize, timeout: Duration) -> PlatformRequestBatch {
        let limit = limit.clamp(1, MAX_PLATFORM_DRAIN_ITEMS);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.pending.is_empty() && !timeout.is_zero() {
            let deadline = Instant::now() + timeout;
            while state.pending.is_empty() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let (next, wait) = self
                    .pending_changed
                    .wait_timeout(state, remaining)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state = next;
                if wait.timed_out() {
                    break;
                }
            }
        }

        let mut requests = Vec::with_capacity(limit);
        while requests.len() < limit {
            let Some(request) = state.pending.pop_front() else {
                break;
            };
            state.in_flight.insert(request.request_id.clone());
            requests.push(request);
        }
        PlatformRequestBatch {
            requests,
            has_more: !state.pending.is_empty(),
        }
    }

    /// Complete one request received from Kotlin.
    pub fn submit_response(&self, response: PlatformResponse) -> Result<(), PlatformPortError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.in_flight.remove(&response.request_id) {
            return Err(PlatformPortError::UnknownRequest);
        }
        state
            .responses
            .insert(response.request_id.clone(), response);
        drop(state);
        self.response_changed.notify_waiters();
        Ok(())
    }

    /// Wait for one response without blocking Tokio worker threads.
    pub async fn wait_for_response(
        &self,
        request_id: &str,
        timeout: Duration,
        cancellation: CancellationToken,
    ) -> Result<Value, PlatformPortError> {
        let request_id = request_id.to_owned();
        let wait = async {
            loop {
                if let Some(response) = self.take_response(&request_id) {
                    return match response.status {
                        PlatformResponseStatus::Succeeded => Ok(response.payload),
                        PlatformResponseStatus::Failed => Err(PlatformPortError::Failed(
                            response
                                .error_code
                                .or(response.error_message)
                                .unwrap_or_else(|| "platform_failure".to_string()),
                        )),
                    };
                }
                tokio::select! {
                    _ = cancellation.cancelled() => return Err(PlatformPortError::Cancelled),
                    _ = self.response_changed.notified() => {}
                }
            }
        };
        let outcome = match tokio::time::timeout(timeout, wait).await {
            Ok(outcome) => outcome,
            Err(_) => Err(PlatformPortError::Timeout),
        };
        if outcome.is_err() {
            self.abandon(&request_id);
        }
        outcome
    }

    /// Remove a request from any broker state after cancellation or timeout.
    pub fn abandon(&self, request_id: &str) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let pending_before = state.pending.len();
        state
            .pending
            .retain(|request| request.request_id != request_id);
        let removed_pending = state.pending.len() != pending_before;
        let removed_in_flight = state.in_flight.remove(request_id);
        let removed_response = state.responses.remove(request_id).is_some();
        removed_pending || removed_in_flight || removed_response
    }

    /// Remove a completed response from the broker.
    pub fn take_response(&self, request_id: &str) -> Option<PlatformResponse> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .responses
            .remove(request_id)
    }

    /// Content-free broker state for diagnostics and snapshots.
    pub fn snapshot(&self) -> PlatformBrokerSnapshot {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        PlatformBrokerSnapshot {
            queued: state.pending.len(),
            in_flight: state.in_flight.len(),
            completed: state.responses.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_moves_request_to_in_flight_and_response_is_single_use() {
        let broker = PlatformRequestBroker::new(4);
        let id = broker
            .enqueue(None, PlatformRequestKind::ThermalStatus)
            .expect("enqueue");
        let batch = broker.drain(1, Duration::ZERO);
        assert_eq!(batch.requests[0].request_id, id);
        assert_eq!(broker.snapshot().in_flight, 1);
        broker
            .submit_response(PlatformResponse {
                request_id: id.clone(),
                status: PlatformResponseStatus::Succeeded,
                payload: serde_json::json!({"status": 0}),
                error_code: None,
                error_message: None,
            })
            .expect("response");
        assert!(broker.take_response(&id).is_some());
        assert!(broker.take_response(&id).is_none());
    }

    #[test]
    fn abandon_removes_pending_request() {
        let broker = PlatformRequestBroker::new(1);
        let id = broker
            .enqueue(None, PlatformRequestKind::NetworkStatus)
            .expect("enqueue");
        assert!(broker.abandon(&id));
        assert!(broker.drain(1, Duration::ZERO).requests.is_empty());
        assert_eq!(broker.snapshot().queued, 0);
    }

    #[test]
    fn rejects_unknown_response() {
        let broker = PlatformRequestBroker::new(1);
        let result = broker.submit_response(PlatformResponse {
            request_id: "unknown".into(),
            status: PlatformResponseStatus::Failed,
            payload: Value::Null,
            error_code: Some("unknown".into()),
            error_message: None,
        });
        assert_eq!(result, Err(PlatformPortError::UnknownRequest));
    }
}
