//! Production-oriented SaaS cloud transport boundary.
//!
//! This module deliberately owns transport concerns only: endpoint validation,
//! shared client policy, authentication injection, request metadata, bounded
//! retries, concurrency, circuit state, and common JSON envelope parsing.
//! Concrete Mukei cloud endpoints and business semantics belong in higher-level
//! services.

use std::{
    collections::BTreeMap,
    fmt,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::StreamExt;
use parking_lot::Mutex;
use reqwest::{
    header::{
        HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER,
    },
    Method, StatusCode, Url,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use super::RetryPolicy;

const JSON_CONTENT_TYPE: &str = "application/json";
const HEADER_API_VERSION: &str = "api_version";
const HEADER_TENANT_ID: &str = "tenant_id";
const HEADER_WORKSPACE_ID: &str = "workspace_id";
const HEADER_ACTOR_ID: &str = "actor_id";
const HEADER_REQUEST_ID: &str = "request_id";
const HEADER_OPERATION_ID: &str = "operation_id";
const HEADER_CORRELATION_ID: &str = "correlation_id";
const HEADER_IDEMPOTENCY_KEY: &str = "idempotency_key";
const DEFAULT_MAX_ERROR_MESSAGE_BYTES: usize = 512;
const DEFAULT_MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_SAAS_REDIRECTS: usize = 10;
const MAX_SAAS_RETRY_ATTEMPTS: u32 = 8;
const MAX_SAAS_RESPONSE_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_SAAS_IN_FLIGHT_REQUESTS: usize = 256;
const MAX_SAAS_TOTAL_REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_SAAS_RETRY_DELAY: Duration = Duration::from_secs(60);
const MAX_SAAS_CIRCUIT_OPEN_DURATION: Duration = Duration::from_secs(5 * 60);

/// Deployment label attached to a validated SaaS endpoint.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SaasEnvironment {
    /// Production cloud service.
    Production,
    /// Pre-production staging service.
    Staging,
    /// Developer-controlled service.
    Development,
}

/// A validated SaaS base endpoint and API version selector.
#[derive(Clone, PartialEq, Eq)]
pub struct SaasEndpoint {
    base_url: Url,
    api_base_url: Url,
    api_version: String,
    environment: SaasEnvironment,
    allow_insecure_http: bool,
}

impl SaasEndpoint {
    /// Validate a cloud endpoint. HTTPS is required unless insecure HTTP is
    /// explicitly enabled by the caller.
    pub fn new(
        base_url: &str,
        api_version_prefix: impl Into<String>,
        environment: SaasEnvironment,
        allow_insecure_http: bool,
    ) -> Result<Self, SaasTransportError> {
        let parsed = Url::parse(base_url)
            .map_err(|_| SaasTransportError::InvalidEndpoint("malformed base URL"))?;

        if parsed.username() != "" || parsed.password().is_some() {
            return Err(SaasTransportError::InvalidEndpoint(
                "embedded URL credentials are forbidden",
            ));
        }
        if parsed.host_str().is_none() {
            return Err(SaasTransportError::InvalidEndpoint(
                "base URL must contain a host",
            ));
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(SaasTransportError::InvalidEndpoint(
                "base URL must not contain a query or fragment",
            ));
        }
        match parsed.scheme() {
            "https" => {}
            "http" if allow_insecure_http => {}
            "http" => {
                return Err(SaasTransportError::InvalidEndpoint(
                    "insecure HTTP is disabled",
                ))
            }
            _ => {
                return Err(SaasTransportError::InvalidEndpoint(
                    "only HTTP(S) endpoints are supported",
                ))
            }
        }

        let api_version = api_version_prefix.into();
        validate_api_version(&api_version)?;
        let mut api_base_url = parsed.clone();
        {
            let mut segments = api_base_url.path_segments_mut().map_err(|_| {
                SaasTransportError::InvalidEndpoint("base URL cannot contain path segments")
            })?;
            segments.pop_if_empty();
            for segment in api_version.split('/') {
                segments.push(segment);
            }
            segments.push("");
        }

        Ok(Self {
            base_url: parsed,
            api_base_url,
            api_version,
            environment,
            allow_insecure_http,
        })
    }

    /// Original validated base URL.
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// API version selector used both in the URL prefix and protocol header.
    pub fn api_version(&self) -> &str {
        &self.api_version
    }

    /// Deployment environment label.
    pub fn environment(&self) -> SaasEnvironment {
        self.environment
    }

    /// Whether explicit insecure HTTP was permitted for this endpoint.
    pub fn allows_insecure_http(&self) -> bool {
        self.allow_insecure_http
    }

    /// Resolve a future service-relative target without allowing an absolute
    /// URL, authority switch, fragment, or `..` escape from the API prefix.
    pub fn resolve(&self, relative_target: &str) -> Result<Url, SaasTransportError> {
        let lower_target = relative_target.to_ascii_lowercase();
        if relative_target.is_empty()
            || relative_target.starts_with('/')
            || relative_target.starts_with("//")
            || relative_target.contains('#')
            || relative_target.contains('\\')
            || lower_target.contains("%2e")
            || relative_target
                .split(|c| c == '/' || c == '?')
                .any(|segment| segment == "..")
        {
            return Err(SaasTransportError::InvalidRequestTarget);
        }

        let resolved = self
            .api_base_url
            .join(relative_target)
            .map_err(|_| SaasTransportError::InvalidRequestTarget)?;
        if !same_origin(&self.api_base_url, &resolved)
            || !resolved.path().starts_with(self.api_base_url.path())
        {
            return Err(SaasTransportError::InvalidRequestTarget);
        }
        Ok(resolved)
    }
}

impl fmt::Debug for SaasEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaasEndpoint")
            .field("base_origin", &safe_endpoint_origin(&self.base_url))
            .field("api_version", &self.api_version)
            .field("environment", &self.environment)
            .field("allow_insecure_http", &self.allow_insecure_http)
            .finish()
    }
}

/// Which semantic classes may use automatic transport retries.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AutomaticRetryPolicy {
    /// Retry safe reads on transient failures.
    pub safe_read: bool,
    /// Retry explicitly idempotent mutations on transient failures.
    pub idempotent_mutation: bool,
    /// Retry non-idempotent mutations. This must remain false; policy
    /// validation rejects unsafe automatic replay.
    pub non_idempotent_mutation: bool,
    /// Retry streaming requests. Kept independent from normal JSON APIs.
    pub streaming_download: bool,
}

impl Default for AutomaticRetryPolicy {
    fn default() -> Self {
        Self {
            safe_read: true,
            idempotent_mutation: true,
            non_idempotent_mutation: false,
            streaming_download: false,
        }
    }
}

/// Optional circuit-breaker policy owned by one shared SaaS client.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SaasCircuitPolicy {
    /// Consecutive transient logical-request failures before opening.
    pub failure_threshold: u32,
    /// Bounded duration for which the circuit remains open before one probe.
    pub open_duration: Duration,
}

impl Default for SaasCircuitPolicy {
    fn default() -> Self {
        Self {
            failure_threshold: 4,
            open_duration: Duration::from_secs(15),
        }
    }
}

/// Shared transport policy for ordinary SaaS JSON APIs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaasClientPolicy {
    /// TCP/TLS connection timeout.
    pub connect_timeout: Duration,
    /// Read inactivity timeout.
    pub read_timeout: Duration,
    /// Total logical request budget including permit waits and retries.
    pub total_request_timeout: Duration,
    /// Maximum number of redirects, restricted to the configured origin.
    pub max_redirects: usize,
    /// Bounded exponential retry policy.
    pub retry_policy: RetryPolicy,
    /// Maximum normal JSON response body size.
    pub max_response_body_bytes: usize,
    /// Maximum concurrent logical SaaS requests owned by this client.
    pub max_in_flight_requests: usize,
    /// Whether future request-body compression may be enabled.
    pub allow_request_compression: bool,
    /// Per-semantic-class automatic retry switches.
    pub automatic_retry: AutomaticRetryPolicy,
    /// Maximum server-provided Retry-After delay accepted by the client.
    pub max_retry_after: Duration,
    /// Optional client-local circuit breaker.
    pub circuit_breaker: Option<SaasCircuitPolicy>,
}

impl Default for SaasClientPolicy {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            total_request_timeout: Duration::from_secs(60),
            max_redirects: 3,
            retry_policy: RetryPolicy {
                max_attempts: 3,
                base_delay: Duration::from_millis(400),
                max_delay: Duration::from_secs(8),
            },
            max_response_body_bytes: 2 * 1024 * 1024,
            max_in_flight_requests: 8,
            allow_request_compression: false,
            automatic_retry: AutomaticRetryPolicy::default(),
            max_retry_after: Duration::from_secs(30),
            circuit_breaker: Some(SaasCircuitPolicy::default()),
        }
    }
}

impl SaasClientPolicy {
    fn validate(&self) -> Result<(), SaasTransportError> {
        if self.connect_timeout.is_zero()
            || self.read_timeout.is_zero()
            || self.total_request_timeout.is_zero()
            || self.total_request_timeout > MAX_SAAS_TOTAL_REQUEST_TIMEOUT
            || self.max_redirects > MAX_SAAS_REDIRECTS
            || self.retry_policy.max_attempts > MAX_SAAS_RETRY_ATTEMPTS
            || self.retry_policy.max_delay > MAX_SAAS_RETRY_DELAY
            || self.max_response_body_bytes == 0
            || self.max_response_body_bytes > MAX_SAAS_RESPONSE_BODY_BYTES
            || self.max_in_flight_requests == 0
            || self.max_in_flight_requests > MAX_SAAS_IN_FLIGHT_REQUESTS
            || self.max_retry_after.is_zero()
            || self.max_retry_after > MAX_SAAS_RETRY_DELAY
            || self.automatic_retry.non_idempotent_mutation
        {
            return Err(SaasTransportError::InvalidPolicy);
        }
        if let Some(circuit) = self.circuit_breaker {
            if circuit.failure_threshold == 0
                || circuit.open_duration.is_zero()
                || circuit.open_duration > MAX_SAAS_CIRCUIT_OPEN_DURATION
            {
                return Err(SaasTransportError::InvalidPolicy);
            }
        }
        Ok(())
    }

    fn retries_allowed_for(&self, class: SaasRequestClass) -> bool {
        match class {
            SaasRequestClass::SafeRead => self.automatic_retry.safe_read,
            SaasRequestClass::IdempotentMutation => self.automatic_retry.idempotent_mutation,
            SaasRequestClass::NonIdempotentMutation => {
                // This class is never automatically replayed by this transport.
                false
            }
            SaasRequestClass::StreamingDownload => self.automatic_retry.streaming_download,
        }
    }
}

/// Secret-bearing outbound access credential for exactly one logical request.
pub struct AccessCredential {
    scheme: String,
    secret: Zeroizing<String>,
}

impl AccessCredential {
    /// Construct an authorization credential such as a Bearer token.
    pub fn new(
        scheme: impl Into<String>,
        secret: impl Into<String>,
    ) -> Result<Self, SaasTransportError> {
        let scheme = scheme.into();
        let secret = secret.into();
        if scheme.is_empty()
            || secret.is_empty()
            || !scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            || contains_invalid_header_controls(&secret)
        {
            return Err(SaasTransportError::InvalidCredential);
        }
        Ok(Self {
            scheme,
            secret: Zeroizing::new(secret),
        })
    }

    fn authorization_header(&self) -> Result<HeaderValue, SaasTransportError> {
        let combined = Zeroizing::new(format!("{} {}", self.scheme, self.secret.as_str()));
        let mut value = HeaderValue::from_str(combined.as_str())
            .map_err(|_| SaasTransportError::InvalidCredential)?;
        value.set_sensitive(true);
        Ok(value)
    }
}

impl fmt::Debug for AccessCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessCredential")
            .field("scheme", &self.scheme)
            .field("secret", &"[redacted-secret]")
            .finish()
    }
}

/// Result of asking an authentication provider for an outbound credential.
#[derive(Debug)]
pub enum CredentialAvailability {
    /// A short-lived access credential is available for this request.
    Available(AccessCredential),
    /// Credential acquisition may succeed later, for example while another
    /// refresh operation is coordinating.
    TemporarilyUnavailable,
    /// The user/session is permanently unauthenticated until higher-level UX
    /// performs authentication again.
    Unauthenticated,
}

/// Provider-neutral authentication acquisition failure.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AuthProviderError {
    /// Authentication provider is temporarily unavailable.
    TemporarilyUnavailable,
    /// Authentication state is permanently unauthenticated.
    Unauthenticated,
    /// Provider failed without exposing implementation or secret details.
    Failed,
}

/// Asynchronous provider-neutral source of short-lived outbound credentials.
#[async_trait]
pub trait AccessTokenProvider: Send + Sync {
    /// Obtain one credential representation for one logical outbound request.
    async fn access_credential(
        &self,
    ) -> Result<CredentialAvailability, AuthProviderError>;
}

/// Canonical opaque metadata reused across every retry of one logical request.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SaasRequestContext {
    tenant_id: Option<String>,
    workspace_id: Option<String>,
    actor_id: Option<String>,
    request_id: Option<String>,
    operation_id: Option<String>,
    correlation_id: Option<String>,
    idempotency_key: Option<String>,
}

impl SaasRequestContext {
    /// Create an empty request context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set opaque tenant metadata.
    pub fn set_tenant_id(&mut self, value: impl Into<String>) -> Result<(), SaasTransportError> {
        self.tenant_id = Some(validate_metadata(value.into())?);
        Ok(())
    }

    /// Set opaque workspace metadata.
    pub fn set_workspace_id(
        &mut self,
        value: impl Into<String>,
    ) -> Result<(), SaasTransportError> {
        self.workspace_id = Some(validate_metadata(value.into())?);
        Ok(())
    }

    /// Set opaque actor metadata.
    pub fn set_actor_id(&mut self, value: impl Into<String>) -> Result<(), SaasTransportError> {
        self.actor_id = Some(validate_metadata(value.into())?);
        Ok(())
    }

    /// Set opaque request identity.
    pub fn set_request_id(&mut self, value: impl Into<String>) -> Result<(), SaasTransportError> {
        self.request_id = Some(validate_metadata(value.into())?);
        Ok(())
    }

    /// Set opaque operation identity.
    pub fn set_operation_id(
        &mut self,
        value: impl Into<String>,
    ) -> Result<(), SaasTransportError> {
        self.operation_id = Some(validate_metadata(value.into())?);
        Ok(())
    }

    /// Set opaque correlation identity.
    pub fn set_correlation_id(
        &mut self,
        value: impl Into<String>,
    ) -> Result<(), SaasTransportError> {
        self.correlation_id = Some(validate_metadata(value.into())?);
        Ok(())
    }

    /// Set the idempotency identity that must remain stable across retries.
    pub fn set_idempotency_key(
        &mut self,
        value: impl Into<String>,
    ) -> Result<(), SaasTransportError> {
        self.idempotency_key = Some(validate_metadata(value.into())?);
        Ok(())
    }

    /// Tenant identity, when present.
    pub fn tenant_id(&self) -> Option<&str> {
        self.tenant_id.as_deref()
    }

    /// Workspace identity, when present.
    pub fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }

    /// Actor identity, when present.
    pub fn actor_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }

    /// Request identity, when present.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// Operation identity, when present.
    pub fn operation_id(&self) -> Option<&str> {
        self.operation_id.as_deref()
    }

    /// Correlation identity, when present.
    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// Idempotency identity, when present.
    pub fn idempotency_key(&self) -> Option<&str> {
        self.idempotency_key.as_deref()
    }
}

impl fmt::Debug for SaasRequestContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaasRequestContext")
            .field("tenant_id", &self.tenant_id)
            .field("workspace_id", &self.workspace_id)
            .field("actor_id", &self.actor_id)
            .field("request_id", &self.request_id)
            .field("operation_id", &self.operation_id)
            .field("correlation_id", &self.correlation_id)
            .field(
                "idempotency_key",
                &self.idempotency_key.as_ref().map(|_| "[present]"),
            )
            .finish()
    }
}

/// Semantic replay class for an outbound request.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SaasRequestClass {
    /// Safe read operation.
    SafeRead,
    /// Mutation whose protocol contract requires a stable idempotency key.
    IdempotentMutation,
    /// Mutation with no automatic replay safety.
    NonIdempotentMutation,
    /// Streaming/download operation with policy independent from normal JSON.
    StreamingDownload,
}

/// Supported mutation verbs without conflating HTTP method and replay safety.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SaasMutationMethod {
    /// POST mutation.
    Post,
    /// PUT mutation.
    Put,
    /// PATCH mutation.
    Patch,
    /// DELETE mutation.
    Delete,
}

impl SaasMutationMethod {
    fn as_reqwest(self) -> Method {
        match self {
            Self::Post => Method::POST,
            Self::Put => Method::PUT,
            Self::Patch => Method::PATCH,
            Self::Delete => Method::DELETE,
        }
    }
}

/// Safe diagnostic projection of an outbound request.
#[derive(Clone, PartialEq, Eq)]
pub struct SaasRequestSummary {
    /// Semantic request class.
    pub class: SaasRequestClass,
    /// Path only; query parameters are intentionally omitted.
    pub path: String,
    /// Stable request identity, when present.
    pub request_id: Option<String>,
    /// Stable operation identity, when present.
    pub operation_id: Option<String>,
    /// Stable correlation identity, when present.
    pub correlation_id: Option<String>,
    /// Whether an idempotency key exists, never its value.
    pub has_idempotency_key: bool,
}

impl fmt::Debug for SaasRequestSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaasRequestSummary")
            .field("class", &self.class)
            .field("path", &self.path)
            .field("request_id", &self.request_id)
            .field("operation_id", &self.operation_id)
            .field("correlation_id", &self.correlation_id)
            .field("has_idempotency_key", &self.has_idempotency_key)
            .finish()
    }
}

/// Common metadata parsed once from a successful SaaS response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaasResponseMetadata {
    /// Server-echoed request identity.
    pub request_id: Option<String>,
    /// Server operation identity.
    pub operation_id: Option<String>,
    /// Server correlation identity.
    pub correlation_id: Option<String>,
    /// Server timestamp as an opaque protocol string.
    pub server_timestamp: Option<String>,
    /// Server API version.
    pub api_version: Option<String>,
}

/// Typed successful SaaS response envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaasResponse<T> {
    /// Common transport metadata.
    pub metadata: SaasResponseMetadata,
    /// Typed business payload owned by the future higher-level service.
    pub payload: T,
}

/// Sanitized provider-neutral error envelope returned by the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaasErrorEnvelope {
    /// Stable machine-readable code.
    pub code: String,
    /// Bounded human-safe message.
    pub message: String,
    /// Server retryability hint, never sufficient by itself to replay unsafe work.
    pub retryable: Option<bool>,
    /// Server request identity.
    pub request_id: Option<String>,
    /// Server correlation identity.
    pub correlation_id: Option<String>,
    /// Bounded field-level validation messages.
    pub field_errors: BTreeMap<String, String>,
    /// Bounded server retry hint.
    pub retry_after: Option<Duration>,
    /// Opaque server operation-status reference.
    pub operation_status_ref: Option<String>,
}

/// Typed failures produced by the SaaS transport boundary.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum SaasTransportError {
    /// Endpoint configuration failed validation.
    #[error("invalid SaaS endpoint: {0}")]
    InvalidEndpoint(&'static str),
    /// Relative request target attempted to escape the configured API boundary.
    #[error("invalid SaaS request target")]
    InvalidRequestTarget,
    /// SaaS transport policy contains an invalid zero or unbounded setting.
    #[error("invalid SaaS client policy")]
    InvalidPolicy,
    /// Caller metadata cannot be represented as a safe HTTP header.
    #[error("invalid SaaS request metadata")]
    InvalidMetadata,
    /// Idempotent mutation omitted its required replay identity.
    #[error("idempotent mutation requires an idempotency key")]
    MissingIdempotencyKey,
    /// Access credential was malformed.
    #[error("invalid access credential")]
    InvalidCredential,
    /// No access credential is currently available, but acquisition may recover.
    #[error("access credential temporarily unavailable")]
    NoCredentialAvailable,
    /// Higher-level authentication is required before cloud requests can proceed.
    #[error("unauthenticated")]
    Unauthenticated,
    /// Server rejected or expired the presented credential.
    #[error("access credential rejected")]
    CredentialRejected {
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Authenticated actor lacks permission unrelated to tenant/workspace scope.
    #[error("permission denied")]
    PermissionDenied {
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Server explicitly rejected tenant scope.
    #[error("tenant forbidden")]
    TenantForbidden {
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Server explicitly rejected workspace scope.
    #[error("workspace forbidden")]
    WorkspaceForbidden {
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Requested resource does not exist.
    #[error("resource not found")]
    NotFound {
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Request conflicts with current server state and may require reconciliation.
    #[error("request conflict")]
    Conflict {
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Server rejected request validation.
    #[error("request validation failed")]
    ValidationFailure {
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Server returned HTTP 408 for this request.
    #[error("server request timeout (HTTP 408)")]
    RequestTimeout {
        /// Bounded Retry-After hint.
        retry_after: Option<Duration>,
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Server returned HTTP 425 and the replay-safe class may retry later.
    #[error("request was too early (HTTP 425)")]
    TooEarly {
        /// Bounded Retry-After hint.
        retry_after: Option<Duration>,
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Server rate-limited the request.
    #[error("request rate limited")]
    RateLimited {
        /// Bounded Retry-After hint.
        retry_after: Option<Duration>,
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Transient server/infrastructure status.
    #[error("transient server failure (HTTP {status})")]
    TransientServerFailure {
        /// HTTP status code.
        status: u16,
        /// Bounded Retry-After hint.
        retry_after: Option<Duration>,
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Client/server API protocol versions are incompatible.
    #[error("API protocol/version mismatch")]
    ProtocolVersionMismatch {
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Permanent HTTP failure not covered by a narrower category.
    #[error("permanent SaaS HTTP failure (HTTP {status})")]
    PermanentHttpFailure {
        /// HTTP status code.
        status: u16,
        /// Sanitized server envelope, when parseable.
        server: Option<Box<SaasErrorEnvelope>>,
    },
    /// Response could not be decoded as the expected common envelope.
    #[error("malformed SaaS response")]
    MalformedResponse,
    /// Normal JSON body exceeded the configured memory bound.
    #[error("SaaS response body exceeds configured limit of {limit_bytes} bytes")]
    ResponseTooLarge {
        /// Configured size limit.
        limit_bytes: usize,
        /// HTTP status when known.
        status: Option<u16>,
    },
    /// Network connection establishment failed.
    #[error("SaaS connection failed")]
    ConnectionFailed,
    /// Network transport became unavailable outside initial connection setup.
    #[error("SaaS transport unavailable")]
    TransportUnavailable,
    /// Logical request budget expired.
    #[error("SaaS request timed out")]
    Timeout,
    /// Request was explicitly cancelled.
    #[error("SaaS request cancelled")]
    Cancelled,
    /// Client-local circuit is temporarily open after repeated transient failures.
    #[error("SaaS transport circuit is temporarily open")]
    CircuitOpen,
    /// JSON request serialization failed without exposing the request body.
    #[error("failed to serialize SaaS request body")]
    RequestSerializationFailed,
    /// JSON client could not be constructed.
    #[error("failed to construct SaaS HTTP client")]
    ClientConstructionFailed,
}

impl SaasTransportError {
    fn is_transient_for_retry(&self) -> bool {
        match self {
            Self::ConnectionFailed
            | Self::TransportUnavailable
            | Self::Timeout
            | Self::MalformedResponse => true,
            Self::RequestTimeout { server, .. }
            | Self::TooEarly { server, .. }
            | Self::RateLimited { server, .. }
            | Self::TransientServerFailure { server, .. } => {
                !matches!(server.as_ref().and_then(|server| server.retryable), Some(false))
            }
            _ => false,
        }
    }

    fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RequestTimeout { retry_after, .. }
            | Self::TooEarly { retry_after, .. }
            | Self::RateLimited { retry_after, .. }
            | Self::TransientServerFailure { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    fn influences_circuit(&self) -> bool {
        matches!(
            self,
            Self::ConnectionFailed
                | Self::TransportUnavailable
                | Self::Timeout
                | Self::TransientServerFailure { .. }
                | Self::MalformedResponse
        )
    }

    fn proves_server_reachable(&self) -> bool {
        matches!(
            self,
            Self::CredentialRejected { .. }
                | Self::PermissionDenied { .. }
                | Self::TenantForbidden { .. }
                | Self::WorkspaceForbidden { .. }
                | Self::NotFound { .. }
                | Self::Conflict { .. }
                | Self::ValidationFailure { .. }
                | Self::RequestTimeout { .. }
                | Self::TooEarly { .. }
                | Self::RateLimited { .. }
                | Self::ProtocolVersionMismatch { .. }
                | Self::PermanentHttpFailure { .. }
                | Self::ResponseTooLarge { .. }
        )
    }

    /// Sanitized server envelope associated with this failure, when present.
    pub fn server_envelope(&self) -> Option<&SaasErrorEnvelope> {
        match self {
            Self::CredentialRejected { server }
            | Self::PermissionDenied { server }
            | Self::TenantForbidden { server }
            | Self::WorkspaceForbidden { server }
            | Self::NotFound { server }
            | Self::Conflict { server }
            | Self::ValidationFailure { server }
            | Self::RequestTimeout { server, .. }
            | Self::TooEarly { server, .. }
            | Self::RateLimited { server, .. }
            | Self::TransientServerFailure { server, .. }
            | Self::ProtocolVersionMismatch { server }
            | Self::PermanentHttpFailure { server, .. } => server.as_deref(),
            _ => None,
        }
    }
}

/// Cloneable shared SaaS transport client.
#[derive(Clone)]
pub struct SaasClient {
    inner: Arc<SaasClientInner>,
}

struct SaasClientInner {
    endpoint: SaasEndpoint,
    policy: SaasClientPolicy,
    http: reqwest::Client,
    auth_provider: Arc<dyn AccessTokenProvider>,
    semaphore: Arc<Semaphore>,
    circuit: Option<CircuitBreaker>,
}

impl fmt::Debug for SaasClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaasClient")
            .field("endpoint", &self.inner.endpoint)
            .field("policy", &self.inner.policy)
            .field("auth_provider", &"[provider]")
            .field("circuit_breaker", &self.inner.circuit.is_some())
            .finish()
    }
}

impl SaasClient {
    /// Construct one shared HTTP client. Cloning `SaasClient` shares client,
    /// concurrency, auth-provider, and circuit state without process globals.
    pub fn new(
        endpoint: SaasEndpoint,
        policy: SaasClientPolicy,
        auth_provider: Arc<dyn AccessTokenProvider>,
    ) -> Result<Self, SaasTransportError> {
        policy.validate()?;

        let redirect_origin = endpoint.base_url.clone();
        let redirect_api_path = endpoint.api_base_url.path().to_string();
        let max_redirects = policy.max_redirects;
        let redirect = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > max_redirects {
                return attempt.stop();
            }
            if !same_origin(&redirect_origin, attempt.url())
                || !attempt.url().path().starts_with(&redirect_api_path)
            {
                return attempt.stop();
            }
            attempt.follow()
        });

        let http = reqwest::Client::builder()
            .user_agent(concat!("mukei-core/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(policy.connect_timeout)
            .read_timeout(policy.read_timeout)
            .timeout(policy.total_request_timeout)
            .redirect(redirect)
            .build()
            .map_err(|_| SaasTransportError::ClientConstructionFailed)?;

        let circuit = policy.circuit_breaker.map(CircuitBreaker::new);
        let semaphore = Arc::new(Semaphore::new(policy.max_in_flight_requests));

        Ok(Self {
            inner: Arc::new(SaasClientInner {
                endpoint,
                policy,
                http,
                auth_provider,
                semaphore,
                circuit,
            }),
        })
    }

    /// Validated endpoint held by this client.
    pub fn endpoint(&self) -> &SaasEndpoint {
        &self.inner.endpoint
    }

    /// Shared SaaS transport policy.
    pub fn policy(&self) -> &SaasClientPolicy {
        &self.inner.policy
    }

    /// Create a context with a fresh opaque request identity. Higher layers may
    /// add tenant/workspace/actor/operation/correlation metadata without the
    /// transport interpreting ownership relationships.
    pub fn new_request_context(&self) -> SaasRequestContext {
        let mut context = SaasRequestContext::new();
        // UUID textual form is always a valid header value.
        context.request_id = Some(uuid::Uuid::new_v4().to_string());
        context
    }

    /// Produce a query-redacted diagnostic summary without bodies or secrets.
    pub fn request_summary(
        &self,
        target: &str,
        class: SaasRequestClass,
        context: &SaasRequestContext,
    ) -> Result<SaasRequestSummary, SaasTransportError> {
        let url = self.inner.endpoint.resolve(target)?;
        Ok(SaasRequestSummary {
            class,
            path: url.path().to_string(),
            request_id: context.request_id.clone(),
            operation_id: context.operation_id.clone(),
            correlation_id: context.correlation_id.clone(),
            has_idempotency_key: context.idempotency_key.is_some(),
        })
    }

    /// Send a typed GET through the normal bounded SaaS JSON path.
    pub async fn send_json_read<T: DeserializeOwned>(
        &self,
        target: &str,
        context: &SaasRequestContext,
        cancel: Option<&CancellationToken>,
    ) -> Result<SaasResponse<T>, SaasTransportError> {
        self.send_json::<(), T>(
            Method::GET,
            target,
            context,
            SaasRequestClass::SafeRead,
            None,
            cancel,
        )
        .await
    }

    /// Send an idempotent mutation. A stable idempotency key is mandatory and
    /// is reused unchanged across every transport retry.
    pub async fn send_json_idempotent_mutation<B: Serialize, T: DeserializeOwned>(
        &self,
        method: SaasMutationMethod,
        target: &str,
        context: &SaasRequestContext,
        body: &B,
        cancel: Option<&CancellationToken>,
    ) -> Result<SaasResponse<T>, SaasTransportError> {
        if context.idempotency_key.is_none() {
            return Err(SaasTransportError::MissingIdempotencyKey);
        }
        self.send_json(
            method.as_reqwest(),
            target,
            context,
            SaasRequestClass::IdempotentMutation,
            Some(body),
            cancel,
        )
        .await
    }

    /// Send a non-idempotent mutation exactly once. Automatic transport replay
    /// is disabled even when the general retry policy is enabled.
    pub async fn send_json_non_idempotent_mutation<B: Serialize, T: DeserializeOwned>(
        &self,
        method: SaasMutationMethod,
        target: &str,
        context: &SaasRequestContext,
        body: &B,
        cancel: Option<&CancellationToken>,
    ) -> Result<SaasResponse<T>, SaasTransportError> {
        self.send_json(
            method.as_reqwest(),
            target,
            context,
            SaasRequestClass::NonIdempotentMutation,
            Some(body),
            cancel,
        )
        .await
    }

    async fn send_json<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        target: &str,
        context: &SaasRequestContext,
        class: SaasRequestClass,
        body: Option<&B>,
        cancel: Option<&CancellationToken>,
    ) -> Result<SaasResponse<T>, SaasTransportError> {
        let started = Instant::now();
        let deadline = started
            .checked_add(self.inner.policy.total_request_timeout)
            .ok_or(SaasTransportError::Timeout)?;

        self.send_json_after_circuit(method, target, context, class, body, cancel, deadline)
            .await
    }

    async fn send_json_after_circuit<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        target: &str,
        context: &SaasRequestContext,
        class: SaasRequestClass,
        body: Option<&B>,
        cancel: Option<&CancellationToken>,
        deadline: Instant,
    ) -> Result<SaasResponse<T>, SaasTransportError> {
        let url = self.inner.endpoint.resolve(target)?;
        let credential = self.acquire_credential(deadline, cancel).await?;
        let serialized_body = match body {
            Some(body) => Some(
                serde_json::to_vec(body)
                    .map_err(|_| SaasTransportError::RequestSerializationFailed)?,
            ),
            None => None,
        };
        let _permit = self.acquire_permit(deadline, cancel).await?;
        if let Some(circuit) = &self.inner.circuit {
            circuit.before_request()?;
        }

        let result = async {
            let mut retries_completed = 0u32;
            loop {
                // Rebuild security-sensitive headers for each consumed request
                // rather than retaining and cloning an Authorization header map.
                let headers = build_saas_headers(
                    &self.inner.endpoint,
                    context,
                    &credential,
                    serialized_body.is_some(),
                )?;
                let mut request = self
                    .inner
                    .http
                    .request(method.clone(), url.clone())
                    .headers(headers);
                if let Some(body) = &serialized_body {
                    request = request.body(body.clone());
                }

                let response_result = wait_with_budget(request.send(), deadline, cancel).await?;
                let outcome = match response_result {
                    Ok(response) => {
                        self.handle_response::<T>(response, context, deadline, cancel)
                            .await
                    }
                    Err(error) => Err(map_transport_error(error)),
                };

                match outcome {
                    Ok(response) => return Ok(response),
                    Err(error) => {
                        let can_retry = self.inner.policy.retries_allowed_for(class)
                            && retries_completed < self.inner.policy.retry_policy.max_attempts
                            && error.is_transient_for_retry();
                        if !can_retry {
                            return Err(error);
                        }

                        retries_completed = retries_completed.saturating_add(1);
                        let jitter = self
                            .inner
                            .policy
                            .retry_policy
                            .next_delay(retries_completed);
                        let delay = error
                            .retry_after()
                            .map(|hint| hint.min(self.inner.policy.max_retry_after))
                            .unwrap_or(jitter);

                        match wait_with_budget(tokio::time::sleep(delay), deadline, cancel).await {
                            Ok(()) => {}
                            Err(SaasTransportError::Cancelled) => {
                                return Err(SaasTransportError::Cancelled)
                            }
                            Err(SaasTransportError::Timeout) => return Err(error),
                            Err(other) => return Err(other),
                        }
                    }
                }
            }
        }
        .await;

        if let Some(circuit) = &self.inner.circuit {
            match &result {
                Ok(_) => circuit.record_success(),
                Err(error) if error.influences_circuit() => circuit.record_transient_failure(),
                Err(SaasTransportError::Cancelled) => circuit.release_probe_without_signal(),
                Err(error) if error.proves_server_reachable() => circuit.record_reachable_response(),
                Err(_) => circuit.release_probe_without_signal(),
            }
        }

        result
    }

    async fn acquire_credential(
        &self,
        deadline: Instant,
        cancel: Option<&CancellationToken>,
    ) -> Result<AccessCredential, SaasTransportError> {
        let result = wait_with_budget(
            self.inner.auth_provider.access_credential(),
            deadline,
            cancel,
        )
        .await?;
        match result {
            Ok(CredentialAvailability::Available(credential)) => Ok(credential),
            Ok(CredentialAvailability::TemporarilyUnavailable) => {
                Err(SaasTransportError::NoCredentialAvailable)
            }
            Ok(CredentialAvailability::Unauthenticated) => {
                Err(SaasTransportError::Unauthenticated)
            }
            Err(AuthProviderError::TemporarilyUnavailable) => {
                Err(SaasTransportError::NoCredentialAvailable)
            }
            Err(AuthProviderError::Unauthenticated) => Err(SaasTransportError::Unauthenticated),
            Err(AuthProviderError::Failed) => Err(SaasTransportError::NoCredentialAvailable),
        }
    }

    async fn acquire_permit(
        &self,
        deadline: Instant,
        cancel: Option<&CancellationToken>,
    ) -> Result<OwnedSemaphorePermit, SaasTransportError> {
        wait_with_budget(self.inner.semaphore.clone().acquire_owned(), deadline, cancel)
            .await?
            .map_err(|_| SaasTransportError::TransportUnavailable)
    }

    async fn handle_response<T: DeserializeOwned>(
        &self,
        response: reqwest::Response,
        context: &SaasRequestContext,
        deadline: Instant,
        cancel: Option<&CancellationToken>,
    ) -> Result<SaasResponse<T>, SaasTransportError> {
        let status = response.status();
        let retry_after = parse_retry_after(
            response.headers().get(RETRY_AFTER),
            self.inner.policy.max_retry_after,
        );
        let body = read_bounded_body(
            response,
            self.inner.policy.max_response_body_bytes,
            deadline,
            cancel,
        )
        .await?;

        if status.is_success() {
            let wire: SuccessEnvelopeWire<T> =
                serde_json::from_slice(&body).map_err(|_| SaasTransportError::MalformedResponse)?;
            return Ok(SaasResponse {
                metadata: SaasResponseMetadata {
                    request_id: safe_optional_protocol_value(wire.request_id),
                    operation_id: safe_optional_protocol_value(wire.operation_id),
                    correlation_id: safe_optional_protocol_value(wire.correlation_id),
                    server_timestamp: safe_optional_protocol_value(wire.server_timestamp),
                    api_version: safe_optional_protocol_value(wire.api_version),
                },
                payload: wire.payload,
            });
        }

        let server = parse_error_envelope(
            &body,
            retry_after,
            self.inner.policy.max_retry_after,
        );
        Err(map_status_error(status, server, retry_after, context))
    }
}

fn build_saas_headers(
    endpoint: &SaasEndpoint,
    context: &SaasRequestContext,
    credential: &AccessCredential,
    has_json_body: bool,
) -> Result<HeaderMap, SaasTransportError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(JSON_CONTENT_TYPE));
    if has_json_body {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(JSON_CONTENT_TYPE));
    }
    headers.insert(AUTHORIZATION, credential.authorization_header()?);
    insert_header(&mut headers, HEADER_API_VERSION, endpoint.api_version())?;
    insert_optional_header(&mut headers, HEADER_TENANT_ID, context.tenant_id())?;
    insert_optional_header(
        &mut headers,
        HEADER_WORKSPACE_ID,
        context.workspace_id(),
    )?;
    insert_optional_header(&mut headers, HEADER_ACTOR_ID, context.actor_id())?;
    insert_optional_header(&mut headers, HEADER_REQUEST_ID, context.request_id())?;
    insert_optional_header(&mut headers, HEADER_OPERATION_ID, context.operation_id())?;
    insert_optional_header(
        &mut headers,
        HEADER_CORRELATION_ID,
        context.correlation_id(),
    )?;
    insert_optional_header(
        &mut headers,
        HEADER_IDEMPOTENCY_KEY,
        context.idempotency_key(),
    )?;
    Ok(headers)
}

fn insert_optional_header(
    headers: &mut HeaderMap,
    name: &'static str,
    value: Option<&str>,
) -> Result<(), SaasTransportError> {
    if let Some(value) = value {
        insert_header(headers, name, value)?;
    }
    Ok(())
}

fn insert_header(
    headers: &mut HeaderMap,
    name: &'static str,
    value: &str,
) -> Result<(), SaasTransportError> {
    if contains_invalid_header_controls(value) {
        return Err(SaasTransportError::InvalidMetadata);
    }
    let name = HeaderName::from_static(name);
    let value = HeaderValue::from_str(value).map_err(|_| SaasTransportError::InvalidMetadata)?;
    headers.insert(name, value);
    Ok(())
}

#[derive(Deserialize)]
struct SuccessEnvelopeWire<T> {
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    operation_id: Option<String>,
    #[serde(default)]
    correlation_id: Option<String>,
    #[serde(default)]
    server_timestamp: Option<String>,
    #[serde(default)]
    api_version: Option<String>,
    #[serde(alias = "data")]
    payload: T,
}

#[derive(Deserialize)]
struct ErrorEnvelopeFieldsWire {
    code: String,
    message: String,
    #[serde(default)]
    retryable: Option<bool>,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    correlation_id: Option<String>,
    #[serde(default)]
    field_errors: BTreeMap<String, String>,
    #[serde(default)]
    retry_after_seconds: Option<u64>,
    #[serde(default)]
    operation_status_ref: Option<String>,
}

#[derive(Deserialize)]
struct NestedErrorEnvelopeWire {
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    correlation_id: Option<String>,
    error: ErrorEnvelopeFieldsWire,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorEnvelopeWire {
    Nested(NestedErrorEnvelopeWire),
    Flat(ErrorEnvelopeFieldsWire),
}

fn parse_error_envelope(
    body: &[u8],
    header_retry_after: Option<Duration>,
    max_retry_after: Duration,
) -> Option<SaasErrorEnvelope> {
    let wire: ErrorEnvelopeWire = serde_json::from_slice(body).ok()?;
    let fields = match wire {
        ErrorEnvelopeWire::Nested(outer) => {
            let mut fields = outer.error;
            if fields.request_id.is_none() {
                fields.request_id = outer.request_id;
            }
            if fields.correlation_id.is_none() {
                fields.correlation_id = outer.correlation_id;
            }
            fields
        }
        ErrorEnvelopeWire::Flat(fields) => fields,
    };

    let retry_after = fields
        .retry_after_seconds
        .map(Duration::from_secs)
        .map(|hint| hint.min(max_retry_after))
        .or(header_retry_after);
    let field_errors = fields
        .field_errors
        .into_iter()
        .take(64)
        .map(|(field, message)| {
            (
                sanitize_bounded_text(field, DEFAULT_MAX_IDENTIFIER_BYTES),
                sanitize_bounded_text(message, DEFAULT_MAX_ERROR_MESSAGE_BYTES),
            )
        })
        .collect();

    Some(SaasErrorEnvelope {
        code: safe_machine_code(fields.code),
        message: sanitize_bounded_text(fields.message, DEFAULT_MAX_ERROR_MESSAGE_BYTES),
        retryable: fields.retryable,
        request_id: safe_optional_protocol_value(fields.request_id),
        correlation_id: safe_optional_protocol_value(fields.correlation_id),
        field_errors,
        retry_after,
        operation_status_ref: fields
            .operation_status_ref
            .map(|value| sanitize_bounded_text(value, DEFAULT_MAX_IDENTIFIER_BYTES)),
    })
}

fn map_status_error(
    status: StatusCode,
    server: Option<SaasErrorEnvelope>,
    header_retry_after: Option<Duration>,
    _context: &SaasRequestContext,
) -> SaasTransportError {
    let server = server.map(Box::new);
    let protocol_mismatch = server.as_ref().is_some_and(|server| {
        matches!(
            server.code.as_str(),
            "api_version_mismatch" | "protocol_version_mismatch"
        )
    });
    let tenant_forbidden = server.as_ref().is_some_and(|server| {
        matches!(
            server.code.as_str(),
            "tenant_forbidden" | "tenant_scope_forbidden"
        )
    });
    let workspace_forbidden = server.as_ref().is_some_and(|server| {
        matches!(
            server.code.as_str(),
            "workspace_forbidden" | "workspace_scope_forbidden"
        )
    });

    if protocol_mismatch || matches!(status.as_u16(), 426 | 505) {
        return SaasTransportError::ProtocolVersionMismatch { server };
    }

    let retry_after = server
        .as_ref()
        .and_then(|server| server.retry_after)
        .or(header_retry_after);

    match status.as_u16() {
        400 | 422 => SaasTransportError::ValidationFailure { server },
        401 => SaasTransportError::CredentialRejected { server },
        403 if tenant_forbidden => SaasTransportError::TenantForbidden { server },
        403 if workspace_forbidden => SaasTransportError::WorkspaceForbidden { server },
        403 => SaasTransportError::PermissionDenied { server },
        404 => SaasTransportError::NotFound { server },
        408 => SaasTransportError::RequestTimeout {
            retry_after,
            server,
        },
        409 => SaasTransportError::Conflict { server },
        425 => SaasTransportError::TooEarly {
            retry_after,
            server,
        },
        429 => SaasTransportError::RateLimited {
            retry_after,
            server,
        },
        500..=599 => SaasTransportError::TransientServerFailure {
            status: status.as_u16(),
            retry_after,
            server,
        },
        _ => SaasTransportError::PermanentHttpFailure {
            status: status.as_u16(),
            server,
        },
    }
}

fn map_transport_error(error: reqwest::Error) -> SaasTransportError {
    if error.is_timeout() {
        SaasTransportError::Timeout
    } else if error.is_connect() {
        SaasTransportError::ConnectionFailed
    } else if error.is_decode() {
        SaasTransportError::MalformedResponse
    } else {
        // Never surface reqwest's raw string because it may contain URLs.
        SaasTransportError::TransportUnavailable
    }
}

fn parse_retry_after(value: Option<&HeaderValue>, max: Duration) -> Option<Duration> {
    let seconds = value?.to_str().ok()?.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds).min(max))
}

async fn read_bounded_body(
    response: reqwest::Response,
    limit: usize,
    deadline: Instant,
    cancel: Option<&CancellationToken>,
) -> Result<Vec<u8>, SaasTransportError> {
    let status = response.status().as_u16();
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();

    while let Some(next) = wait_with_budget(stream.next(), deadline, cancel).await? {
        let chunk = next.map_err(map_transport_error)?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(SaasTransportError::ResponseTooLarge {
                limit_bytes: limit,
                status: Some(status),
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn wait_with_budget<F, T>(
    future: F,
    deadline: Instant,
    cancel: Option<&CancellationToken>,
) -> Result<T, SaasTransportError>
where
    F: Future<Output = T>,
{
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or(SaasTransportError::Timeout)?;
    if remaining.is_zero() {
        return Err(SaasTransportError::Timeout);
    }

    if let Some(cancel) = cancel {
        tokio::select! {
            _ = cancel.cancelled() => Err(SaasTransportError::Cancelled),
            result = tokio::time::timeout(remaining, future) => {
                result.map_err(|_| SaasTransportError::Timeout)
            }
        }
    } else {
        tokio::time::timeout(remaining, future)
            .await
            .map_err(|_| SaasTransportError::Timeout)
    }
}

fn validate_api_version(value: &str) -> Result<(), SaasTransportError> {
    if value.is_empty()
        || value.len() > DEFAULT_MAX_IDENTIFIER_BYTES
        || value.starts_with('/')
        || value.ends_with('/')
        || value.split('/').any(|segment| {
            segment.is_empty()
                || matches!(segment, "." | "..")
                || contains_invalid_header_controls(segment)
        })
    {
        return Err(SaasTransportError::InvalidEndpoint(
            "invalid API version prefix",
        ));
    }
    HeaderValue::from_str(value)
        .map_err(|_| SaasTransportError::InvalidEndpoint("invalid API version prefix"))?;
    Ok(())
}

fn validate_metadata(value: String) -> Result<String, SaasTransportError> {
    if value.is_empty()
        || value.len() > DEFAULT_MAX_IDENTIFIER_BYTES
        || contains_invalid_header_controls(&value)
    {
        return Err(SaasTransportError::InvalidMetadata);
    }
    HeaderValue::from_str(&value).map_err(|_| SaasTransportError::InvalidMetadata)?;
    Ok(value)
}

fn contains_invalid_header_controls(value: &str) -> bool {
    value
        .bytes()
        .any(|b| b == b'\r' || b == b'\n' || b == 0 || (b < 0x20 && b != b'\t') || b == 0x7f)
}

fn safe_optional_protocol_value(value: Option<String>) -> Option<String> {
    value.and_then(|value| safe_protocol_value(value, DEFAULT_MAX_IDENTIFIER_BYTES))
}

fn safe_protocol_value(value: String, limit: usize) -> Option<String> {
    if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        None
    } else {
        Some(value)
    }
}

fn safe_machine_code(value: String) -> String {
    let Some(value) = safe_protocol_value(value, DEFAULT_MAX_IDENTIFIER_BYTES) else {
        return "unknown_error".to_string();
    };
    let sanitized = crate::diagnostics::sanitize_log_value(&value);
    if sanitized.starts_with("[redacted-") {
        "unknown_error".to_string()
    } else {
        value
    }
}

fn sanitize_bounded_text(value: String, limit: usize) -> String {
    let without_controls: String = value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(limit)
        .collect();
    let query_redacted = without_controls
        .split_whitespace()
        .map(redact_secret_like_token)
        .collect::<Vec<_>>()
        .join(" ");
    crate::diagnostics::sanitize_error_message(query_redacted)
}

fn redact_secret_like_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    if lower.contains("access_token=")
        || lower.contains("token=")
        || lower.contains("api_key=")
        || lower.contains("apikey=")
        || lower.contains("key=")
        || lower.contains("secret=")
        || lower.contains("cookie=")
        || lower.contains("authorization=")
        || lower.contains("signature=")
        || lower.contains("x-amz-signature=")
        || lower.contains("sig=")
    {
        return "[redacted-secret]".to_string();
    }
    if (lower.starts_with("http://") || lower.starts_with("https://"))
        && token.contains('?')
    {
        let prefix = token.split_once('?').map(|(prefix, _)| prefix).unwrap_or(token);
        return format!("{prefix}?[redacted-query]");
    }
    token.to_string()
}

fn safe_endpoint_origin(url: &Url) -> String {
    match (url.host_str(), url.port()) {
        (Some(host), Some(port)) => format!("{}://{}:{}", url.scheme(), host, port),
        (Some(host), None) => format!("{}://{}", url.scheme(), host),
        _ => "[invalid-origin]".to_string(),
    }
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

#[derive(Debug)]
struct CircuitBreaker {
    policy: SaasCircuitPolicy,
    state: Mutex<CircuitState>,
}

#[derive(Copy, Clone, Debug)]
enum CircuitState {
    Closed { consecutive_failures: u32 },
    Open { until: Instant },
    HalfOpen { probe_in_flight: bool },
}

impl CircuitBreaker {
    fn new(policy: SaasCircuitPolicy) -> Self {
        Self {
            policy,
            state: Mutex::new(CircuitState::Closed {
                consecutive_failures: 0,
            }),
        }
    }

    fn before_request(&self) -> Result<(), SaasTransportError> {
        let now = Instant::now();
        let mut state = self.state.lock();
        match *state {
            CircuitState::Closed { .. } => Ok(()),
            CircuitState::Open { until } if now < until => Err(SaasTransportError::CircuitOpen),
            CircuitState::Open { .. } => {
                *state = CircuitState::HalfOpen {
                    probe_in_flight: true,
                };
                Ok(())
            }
            CircuitState::HalfOpen {
                probe_in_flight: true,
            } => Err(SaasTransportError::CircuitOpen),
            CircuitState::HalfOpen {
                probe_in_flight: false,
            } => {
                *state = CircuitState::HalfOpen {
                    probe_in_flight: true,
                };
                Ok(())
            }
        }
    }

    fn record_success(&self) {
        *self.state.lock() = CircuitState::Closed {
            consecutive_failures: 0,
        };
    }

    fn record_reachable_response(&self) {
        // Authentication, permission, conflict, and validation responses prove
        // the infrastructure is reachable and must not trip the circuit.
        self.record_success();
    }

    fn record_transient_failure(&self) {
        let now = Instant::now();
        let mut state = self.state.lock();
        match *state {
            CircuitState::Closed {
                consecutive_failures,
            } => {
                let next = consecutive_failures.saturating_add(1);
                if next >= self.policy.failure_threshold {
                    *state = CircuitState::Open {
                        until: now + self.policy.open_duration,
                    };
                } else {
                    *state = CircuitState::Closed {
                        consecutive_failures: next,
                    };
                }
            }
            CircuitState::Open { .. } | CircuitState::HalfOpen { .. } => {
                *state = CircuitState::Open {
                    until: now + self.policy.open_duration,
                };
            }
        }
    }

    fn release_probe_without_signal(&self) {
        let mut state = self.state.lock();
        if matches!(*state, CircuitState::HalfOpen { .. }) {
            *state = CircuitState::HalfOpen {
                probe_in_flight: false,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_rejects_embedded_credentials_and_http_by_default() {
        assert!(SaasEndpoint::new(
            "https://user:secret@example.com",
            "v1",
            SaasEnvironment::Production,
            false,
        )
        .is_err());
        assert!(SaasEndpoint::new(
            "http://example.com",
            "v1",
            SaasEnvironment::Production,
            false,
        )
        .is_err());
    }

    #[test]
    fn endpoint_resolution_cannot_escape_api_prefix() {
        let endpoint = SaasEndpoint::new(
            "https://api.example.com/root",
            "v1",
            SaasEnvironment::Staging,
            false,
        )
        .unwrap();
        assert!(endpoint.resolve("users?page=1").is_ok());
        assert!(endpoint.resolve("../admin").is_err());
        assert!(endpoint.resolve("https://evil.example/path").is_err());
    }

    #[test]
    fn credential_debug_never_exposes_secret() {
        let credential = AccessCredential::new("Bearer", "top-secret-token").unwrap();
        let debug = format!("{credential:?}");
        assert!(!debug.contains("top-secret-token"));
        assert!(debug.contains("redacted-secret"));
    }

    #[test]
    fn retry_after_seconds_is_bounded() {
        let value = HeaderValue::from_static("9999");
        assert_eq!(
            parse_retry_after(Some(&value), Duration::from_secs(10)),
            Some(Duration::from_secs(10))
        );
        let malformed = HeaderValue::from_static("tomorrow");
        assert_eq!(
            parse_retry_after(Some(&malformed), Duration::from_secs(10)),
            None
        );
    }

    #[test]
    fn non_idempotent_mutation_never_uses_automatic_retry() {
        let policy = SaasClientPolicy::default();
        assert!(!policy.retries_allowed_for(SaasRequestClass::NonIdempotentMutation));
    }

    #[test]
    fn request_context_rejects_control_characters() {
        let mut context = SaasRequestContext::new();
        assert!(context.set_tenant_id("tenant\r\nattack").is_err());
    }
}
