//! Local-first SaaS tenancy, entitlement, subscription, usage, and quota domain.
//!
//! This module is intentionally transport- and UI-agnostic. It models durable
//! ownership boundaries and deterministic decisions that can operate with a
//! local installation scope today and accept authoritative server snapshots in
//! the future.
//!
//! # Core invariants
//!
//! - Tenant, workspace, and actor identifiers are opaque, validated strings.
//! - A local installation scope is deterministic but is never represented as a
//!   remote/cloud tenant.
//! - Entitlement and subscription decisions fail closed when authoritative
//!   state is stale or unavailable.
//! - Usage quantities are non-negative; corrections are separate credit events.
//! - Quota decisions are pure and contain machine-readable reasons only.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Maximum number of structured metadata entries accepted on one usage event.
pub const MAX_USAGE_METADATA_ENTRIES: usize = 16;
/// Maximum UTF-8 byte length of a usage metadata key.
pub const MAX_USAGE_METADATA_KEY_BYTES: usize = 64;
/// Maximum UTF-8 byte length of a usage metadata string value.
pub const MAX_USAGE_METADATA_STRING_BYTES: usize = 256;
/// Maximum serialized JSON byte length of a usage metadata object.
pub const MAX_USAGE_METADATA_JSON_BYTES: usize = 4096;

/// Errors produced by pure SaaS-domain validation and arithmetic.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SaasDomainError {
    /// An opaque identifier was empty or only whitespace.
    #[error("{kind} must not be empty or whitespace-only")]
    EmptyIdentifier {
        /// Machine-readable identifier type name.
        kind: &'static str,
    },
    /// A machine key failed its bounded-key validation.
    #[error("{kind} is invalid: {reason}")]
    InvalidMachineKey {
        /// Machine-readable key type name.
        kind: &'static str,
        /// Stable validation reason.
        reason: &'static str,
    },
    /// A time window was internally inconsistent.
    #[error("invalid usage window: {0}")]
    InvalidUsageWindow(&'static str),
    /// Usage credits exceeded consumed usage for an aggregation result.
    #[error("usage credits exceed consumed usage")]
    UsageUnderflow,
    /// An addition exceeded the supported unsigned usage range.
    #[error("usage arithmetic overflow")]
    UsageOverflow,
    /// Usage metadata exceeded a bounded safety constraint.
    #[error("usage metadata rejected: {0}")]
    UnsafeUsageMetadata(&'static str),
}

macro_rules! opaque_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Construct a validated opaque identifier without normalizing case.
            pub fn new(value: impl Into<String>) -> Result<Self, SaasDomainError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(SaasDomainError::EmptyIdentifier { kind: $kind });
                }
                Ok(Self(value))
            }

            /// Borrow the stable string representation.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume the wrapper and return its stable string representation.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl FromStr for $name {
            type Err = SaasDomainError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value.to_owned())
            }
        }

        impl TryFrom<String> for $name {
            type Error = SaasDomainError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

opaque_id!(
    /// Billing and contract boundary identifier.
    TenantId,
    "tenant_id"
);
opaque_id!(
    /// Collaboration and data-ownership boundary identifier within a tenant.
    WorkspaceId,
    "workspace_id"
);
opaque_id!(
    /// User or service identity performing an action.
    ActorId,
    "actor_id"
);
opaque_id!(
    /// Stable identity of one actor-to-workspace membership.
    MembershipId,
    "membership_id"
);
opaque_id!(
    /// Stable identity of an immutable entitlement snapshot.
    EntitlementSnapshotId,
    "entitlement_snapshot_id"
);
opaque_id!(
    /// Stable identity of one immutable usage ledger event.
    UsageEventId,
    "usage_event_id"
);
opaque_id!(
    /// Stable identity of one provider-neutral subscription snapshot.
    SubscriptionSnapshotId,
    "subscription_snapshot_id"
);
opaque_id!(
    /// Stable identity of one versioned quota policy record.
    QuotaPolicyId,
    "quota_policy_id"
);
opaque_id!(
    /// Identifier for one client request attempt.
    RequestId,
    "request_id"
);
opaque_id!(
    /// Durable logical operation identifier that can outlive one request.
    OperationId,
    "operation_id"
);
opaque_id!(
    /// Cross-component tracing identifier.
    CorrelationId,
    "correlation_id"
);
opaque_id!(
    /// Replay-protection key for one logically unique mutation.
    IdempotencyKey,
    "idempotency_key"
);

/// Monotonic server revision used for deterministic snapshot ordering.
pub type RemoteRevision = u64;

/// Origin of truth for durable tenancy records.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceOfTruth {
    /// Installation-local record that does not claim remote authority.
    Local,
    /// Record issued or managed by a remote authoritative service.
    Remote,
}

impl SourceOfTruth {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

/// Tenant lifecycle status.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantStatus {
    /// Tenant may perform normal operations.
    Active,
    /// Tenant remains durable but access can be restricted.
    Suspended,
    /// Tenant is disabled/closed and is not normally usable.
    Closed,
}

impl TenantStatus {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Closed => "closed",
        }
    }
}

/// Durable tenant record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tenant {
    /// Billing and contract boundary.
    pub tenant_id: TenantId,
    /// Human-readable display label, not an authorization key.
    pub display_name: String,
    /// Current lifecycle status.
    pub status: TenantStatus,
    /// Whether the record is local or remotely authoritative.
    pub source_of_truth: SourceOfTruth,
    /// Creation timestamp in UTC.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp in UTC.
    pub updated_at: DateTime<Utc>,
    /// Optional monotonic remote revision.
    pub remote_revision: Option<RemoteRevision>,
    /// Optional bounded machine reason for suspension.
    pub suspension_reason: Option<String>,
}

/// Workspace lifecycle status.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    /// Workspace may perform normal operations.
    Active,
    /// Workspace remains durable but is temporarily suspended.
    Suspended,
    /// Workspace is disabled without deleting historical ownership.
    Disabled,
}

impl WorkspaceStatus {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Disabled => "disabled",
        }
    }
}

/// Management policy for a workspace.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceManagement {
    /// Installation-local workspace with no remote manager.
    LocalOnly,
    /// Workspace state is expected to be managed by a remote authority.
    RemoteManaged,
}

impl WorkspaceManagement {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalOnly => "local_only",
            Self::RemoteManaged => "remote_managed",
        }
    }
}

/// Durable workspace record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Collaboration/data ownership boundary.
    pub workspace_id: WorkspaceId,
    /// Owning billing/contract boundary.
    pub tenant_id: TenantId,
    /// Human-readable display label.
    pub display_name: String,
    /// Current workspace lifecycle status.
    pub status: WorkspaceStatus,
    /// Creation timestamp in UTC.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp in UTC.
    pub updated_at: DateTime<Utc>,
    /// Optional monotonic remote revision.
    pub remote_revision: Option<RemoteRevision>,
    /// Whether management is local-only or remote.
    pub management: WorkspaceManagement,
}

/// Actor kind without embedding provider-specific identity data.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    /// Local installation actor.
    Local,
    /// Human user identity.
    Human,
    /// Service or automation identity.
    Service,
}

impl ActorKind {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Human => "human",
            Self::Service => "service",
        }
    }
}

/// Actor lifecycle status.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorStatus {
    /// Actor may participate normally.
    Active,
    /// Actor is temporarily suspended.
    Suspended,
    /// Actor is disabled while historical references remain intact.
    Disabled,
}

impl ActorStatus {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Disabled => "disabled",
        }
    }
}

/// Durable actor record kept separate from workspace membership.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    /// User/service identity.
    pub actor_id: ActorId,
    /// Optional human-readable label without provider PII requirements.
    pub display_name: Option<String>,
    /// Stable actor category.
    pub kind: ActorKind,
    /// Current actor lifecycle status.
    pub status: ActorStatus,
    /// Whether the actor record is local or remotely sourced.
    pub source_of_truth: SourceOfTruth,
    /// Creation timestamp in UTC.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp in UTC.
    pub updated_at: DateTime<Utc>,
    /// Optional monotonic remote revision.
    pub remote_revision: Option<RemoteRevision>,
}

/// Stable workspace role hierarchy value.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipRole {
    /// Read-only workspace access.
    ReadOnly,
    /// Normal member access.
    Member,
    /// Administrative workspace access.
    Admin,
    /// Highest ownership role.
    Owner,
}

impl MembershipRole {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
            Self::ReadOnly => "read_only",
        }
    }
}

/// Membership lifecycle status.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipStatus {
    /// Membership may be used normally.
    Active,
    /// Membership is temporarily suspended.
    Suspended,
    /// Membership was revoked but remains for history/audit reasoning.
    Revoked,
}

impl MembershipStatus {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Revoked => "revoked",
        }
    }
}

/// Actor membership in one workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceMembership {
    /// Membership business identity.
    pub membership_id: MembershipId,
    /// Tenant boundary copied explicitly for ownership validation.
    pub tenant_id: TenantId,
    /// Workspace boundary.
    pub workspace_id: WorkspaceId,
    /// Actor identity.
    pub actor_id: ActorId,
    /// Stable authorization role value.
    pub role: MembershipRole,
    /// Membership lifecycle state.
    pub status: MembershipStatus,
    /// Creation timestamp in UTC.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp in UTC.
    pub updated_at: DateTime<Utc>,
    /// Optional monotonic remote revision.
    pub remote_revision: Option<RemoteRevision>,
}

/// Deterministic installation-local tenant/workspace/actor/membership scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalScope {
    /// Deterministic local tenant identifier.
    pub tenant_id: TenantId,
    /// Deterministic local workspace identifier.
    pub workspace_id: WorkspaceId,
    /// Deterministic local actor identifier.
    pub actor_id: ActorId,
    /// Deterministic local owner-membership identifier.
    pub membership_id: MembershipId,
}

impl LocalScope {
    /// Derive a deterministic local scope from a stable installation identifier.
    ///
    /// Each business identifier is independently derived from the installation
    /// seed and a domain separator, so no identifier is inferred from another.
    pub fn for_installation(installation_id: &str) -> Result<Self, SaasDomainError> {
        if installation_id.trim().is_empty() {
            return Err(SaasDomainError::EmptyIdentifier {
                kind: "installation_id",
            });
        }
        Ok(Self {
            tenant_id: TenantId::new(derive_local_identifier("tenant", installation_id))?,
            workspace_id: WorkspaceId::new(derive_local_identifier("workspace", installation_id))?,
            actor_id: ActorId::new(derive_local_identifier("actor", installation_id))?,
            membership_id: MembershipId::new(derive_local_identifier("membership", installation_id))?,
        })
    }

    /// Materialize the local tenant record for first-run persistence.
    pub fn tenant(&self, now: DateTime<Utc>) -> Tenant {
        Tenant {
            tenant_id: self.tenant_id.clone(),
            display_name: "Local installation".to_owned(),
            status: TenantStatus::Active,
            source_of_truth: SourceOfTruth::Local,
            created_at: now,
            updated_at: now,
            remote_revision: None,
            suspension_reason: None,
        }
    }

    /// Materialize the local workspace record for first-run persistence.
    pub fn workspace(&self, now: DateTime<Utc>) -> Workspace {
        Workspace {
            workspace_id: self.workspace_id.clone(),
            tenant_id: self.tenant_id.clone(),
            display_name: "Local workspace".to_owned(),
            status: WorkspaceStatus::Active,
            created_at: now,
            updated_at: now,
            remote_revision: None,
            management: WorkspaceManagement::LocalOnly,
        }
    }

    /// Materialize the local actor record for first-run persistence.
    pub fn actor(&self, now: DateTime<Utc>) -> Actor {
        Actor {
            actor_id: self.actor_id.clone(),
            display_name: Some("Local actor".to_owned()),
            kind: ActorKind::Local,
            status: ActorStatus::Active,
            source_of_truth: SourceOfTruth::Local,
            created_at: now,
            updated_at: now,
            remote_revision: None,
        }
    }

    /// Materialize the local actor's owner membership for first-run persistence.
    pub fn membership(&self, now: DateTime<Utc>) -> WorkspaceMembership {
        WorkspaceMembership {
            membership_id: self.membership_id.clone(),
            tenant_id: self.tenant_id.clone(),
            workspace_id: self.workspace_id.clone(),
            actor_id: self.actor_id.clone(),
            role: MembershipRole::Owner,
            status: MembershipStatus::Active,
            created_at: now,
            updated_at: now,
            remote_revision: None,
        }
    }
}

fn derive_local_identifier(namespace: &str, installation_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"mukei-local-scope-v1\0");
    hasher.update(namespace.as_bytes());
    hasher.update(b"\0");
    hasher.update(installation_id.as_bytes());
    let digest = hasher.finalize();
    format!("local:{namespace}:{digest:x}")
}

/// Provider-neutral subscription lifecycle state.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    /// Paid/contracted access is currently active.
    Active,
    /// Trial access is currently active.
    Trial,
    /// Access remains available during a grace period.
    GracePeriod,
    /// Subscription is past due or otherwise restricted.
    PastDueRestricted,
    /// Subscription is cancelled but may remain effective until a date.
    CancelledEffective,
    /// Subscription is no longer effective.
    Expired,
    /// State is unknown or too stale to trust.
    UnknownStale,
}

impl SubscriptionStatus {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Trial => "trial",
            Self::GracePeriod => "grace_period",
            Self::PastDueRestricted => "past_due_restricted",
            Self::CancelledEffective => "cancelled_effective",
            Self::Expired => "expired",
            Self::UnknownStale => "unknown_stale",
        }
    }
}

/// Whether a snapshot is authoritative or only a local provisional view.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotAuthority {
    /// Snapshot is authoritative for access decisions.
    Authoritative,
    /// Snapshot is local/provisional and must not silently grant premium access.
    LocalProvisional,
}

impl SnapshotAuthority {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authoritative => "authoritative",
            Self::LocalProvisional => "local_provisional",
        }
    }
}

/// Coarse subscription access state used by quota evaluation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionAccessState {
    /// Subscription permits access.
    Allowed,
    /// Subscription is known to restrict access.
    Restricted,
    /// Authoritative subscription state is unknown or stale.
    StaleOrUnknown,
}

/// One versioned provider-neutral subscription snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionState {
    /// Snapshot business identity.
    pub snapshot_id: SubscriptionSnapshotId,
    /// Tenant whose subscription is described.
    pub tenant_id: TenantId,
    /// Provider-neutral stable plan key.
    pub plan_key: String,
    /// Current subscription status.
    pub status: SubscriptionStatus,
    /// Effective start timestamp.
    pub effective_start: DateTime<Utc>,
    /// Optional effective end timestamp.
    pub effective_end: Option<DateTime<Utc>>,
    /// Optional grace-period end timestamp.
    pub grace_period_end: Option<DateTime<Utc>>,
    /// Optional monotonic source revision.
    pub source_revision: Option<RemoteRevision>,
    /// Last successful synchronization timestamp.
    pub last_synced_at: DateTime<Utc>,
    /// Whether this snapshot is authoritative or locally provisional.
    pub authority: SnapshotAuthority,
    /// Optional explicit stale marker without deleting the snapshot.
    pub stale_marked_at: Option<DateTime<Utc>>,
}

impl SubscriptionState {
    /// Return whether the snapshot is stale or untrusted at `at`.
    pub fn is_stale(&self, at: DateTime<Utc>, stale_after: Duration) -> bool {
        self.authority != SnapshotAuthority::Authoritative
            || self.stale_marked_at.is_some()
            || self.status == SubscriptionStatus::UnknownStale
            || at.signed_duration_since(self.last_synced_at) > stale_after
    }

    /// Resolve access with an explicit maximum trusted synchronization age.
    pub fn access_state_with_staleness(
        &self,
        at: DateTime<Utc>,
        stale_after: Duration,
    ) -> SubscriptionAccessState {
        if self.is_stale(at, stale_after) {
            SubscriptionAccessState::StaleOrUnknown
        } else {
            self.access_state(at)
        }
    }

    /// Resolve subscription access conservatively at the supplied time.
    pub fn access_state(&self, at: DateTime<Utc>) -> SubscriptionAccessState {
        if self.authority != SnapshotAuthority::Authoritative || self.stale_marked_at.is_some() {
            return SubscriptionAccessState::StaleOrUnknown;
        }
        match self.status {
            SubscriptionStatus::Active | SubscriptionStatus::Trial => {
                if self.effective_end.is_some_and(|end| at >= end) {
                    SubscriptionAccessState::Restricted
                } else {
                    SubscriptionAccessState::Allowed
                }
            }
            SubscriptionStatus::GracePeriod => {
                if self.grace_period_end.is_some_and(|end| at >= end) {
                    SubscriptionAccessState::Restricted
                } else {
                    SubscriptionAccessState::Allowed
                }
            }
            SubscriptionStatus::CancelledEffective => {
                if self.effective_end.is_some_and(|end| at < end) {
                    SubscriptionAccessState::Allowed
                } else {
                    SubscriptionAccessState::Restricted
                }
            }
            SubscriptionStatus::PastDueRestricted | SubscriptionStatus::Expired => {
                SubscriptionAccessState::Restricted
            }
            SubscriptionStatus::UnknownStale => SubscriptionAccessState::StaleOrUnknown,
        }
    }
}

/// Stable data-driven entitlement key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct EntitlementKey(String);

impl EntitlementKey {
    /// Construct a bounded machine entitlement key.
    pub fn new(value: impl Into<String>) -> Result<Self, SaasDomainError> {
        Ok(Self(validate_machine_key(
            value.into(),
            "entitlement_key",
            128,
        )?))
    }

    /// Borrow the stable key representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntitlementKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for EntitlementKey {
    type Err = SaasDomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value.to_owned())
    }
}

impl<'de> Deserialize<'de> for EntitlementKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// One feature grant within an immutable entitlement snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitlementGrant {
    /// Data-driven entitlement key.
    pub key: EntitlementKey,
    /// Explicit enabled/disabled state.
    pub enabled: bool,
    /// Optional non-negative numeric limit.
    pub numeric_limit: Option<u64>,
    /// Optional bounded string value.
    pub string_value: Option<String>,
    /// Optional grant effective start.
    pub effective_start: Option<DateTime<Utc>>,
    /// Optional grant effective end.
    pub effective_end: Option<DateTime<Utc>>,
    /// Optional source revision for this grant.
    pub source_revision: Option<RemoteRevision>,
}

/// Immutable versioned entitlement snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitlementSnapshot {
    /// Snapshot business identity.
    pub snapshot_id: EntitlementSnapshotId,
    /// Tenant described by this snapshot.
    pub tenant_id: TenantId,
    /// Snapshot grants keyed by stable entitlement key.
    pub grants: BTreeMap<EntitlementKey, EntitlementGrant>,
    /// Optional monotonic source revision.
    pub source_revision: Option<RemoteRevision>,
    /// Last synchronization/observation timestamp.
    pub last_synced_at: DateTime<Utc>,
    /// Whether decisions may trust this snapshot.
    pub authority: SnapshotAuthority,
    /// Optional stale marker without destructive deletion.
    pub stale_marked_at: Option<DateTime<Utc>>,
}

/// Result of an entitlement lookup without collapsing unknown into granted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum EntitlementDecision<T> {
    /// Entitlement is explicitly granted with the resolved value.
    Granted(T),
    /// Entitlement is explicitly denied or outside its effective interval.
    Denied,
    /// Entitlement/key/value is not present in the snapshot.
    Unknown,
    /// Snapshot exists but is stale or not authoritative enough to trust.
    StaleUntrusted,
}

impl EntitlementSnapshot {
    /// Return whether the snapshot is stale or untrusted at `at`.
    pub fn is_stale(&self, at: DateTime<Utc>, stale_after: Duration) -> bool {
        self.authority != SnapshotAuthority::Authoritative
            || self.stale_marked_at.is_some()
            || at.signed_duration_since(self.last_synced_at) > stale_after
    }

    /// Resolve whether a feature is enabled.
    pub fn feature_enabled(
        &self,
        key: &EntitlementKey,
        at: DateTime<Utc>,
        stale_after: Duration,
    ) -> EntitlementDecision<bool> {
        match self.resolve_grant(key, at, stale_after) {
            EntitlementDecision::Granted(grant) => EntitlementDecision::Granted(grant.enabled),
            EntitlementDecision::Denied => EntitlementDecision::Denied,
            EntitlementDecision::Unknown => EntitlementDecision::Unknown,
            EntitlementDecision::StaleUntrusted => EntitlementDecision::StaleUntrusted,
        }
    }

    /// Resolve a feature entitlement as a unit-valued gate for quota checks.
    pub fn access_gate(
        &self,
        key: &EntitlementKey,
        at: DateTime<Utc>,
        stale_after: Duration,
    ) -> EntitlementDecision<()> {
        match self.feature_enabled(key, at, stale_after) {
            EntitlementDecision::Granted(true) => EntitlementDecision::Granted(()),
            EntitlementDecision::Granted(false) | EntitlementDecision::Denied => {
                EntitlementDecision::Denied
            }
            EntitlementDecision::Unknown => EntitlementDecision::Unknown,
            EntitlementDecision::StaleUntrusted => EntitlementDecision::StaleUntrusted,
        }
    }

    /// Resolve the numeric limit for an entitlement key.
    pub fn numeric_limit(
        &self,
        key: &EntitlementKey,
        at: DateTime<Utc>,
        stale_after: Duration,
    ) -> EntitlementDecision<u64> {
        match self.resolve_grant(key, at, stale_after) {
            EntitlementDecision::Granted(grant) if grant.enabled => grant
                .numeric_limit
                .map(EntitlementDecision::Granted)
                .unwrap_or(EntitlementDecision::Unknown),
            EntitlementDecision::Granted(_) | EntitlementDecision::Denied => {
                EntitlementDecision::Denied
            }
            EntitlementDecision::Unknown => EntitlementDecision::Unknown,
            EntitlementDecision::StaleUntrusted => EntitlementDecision::StaleUntrusted,
        }
    }

    /// Resolve the string value for an entitlement key.
    pub fn string_value(
        &self,
        key: &EntitlementKey,
        at: DateTime<Utc>,
        stale_after: Duration,
    ) -> EntitlementDecision<String> {
        match self.resolve_grant(key, at, stale_after) {
            EntitlementDecision::Granted(grant) if grant.enabled => grant
                .string_value
                .clone()
                .map(EntitlementDecision::Granted)
                .unwrap_or(EntitlementDecision::Unknown),
            EntitlementDecision::Granted(_) | EntitlementDecision::Denied => {
                EntitlementDecision::Denied
            }
            EntitlementDecision::Unknown => EntitlementDecision::Unknown,
            EntitlementDecision::StaleUntrusted => EntitlementDecision::StaleUntrusted,
        }
    }

    /// Return whether a known grant has passed its effective end.
    pub fn entitlement_expired(&self, key: &EntitlementKey, at: DateTime<Utc>) -> bool {
        self.grants
            .get(key)
            .and_then(|grant| grant.effective_end)
            .is_some_and(|end| at >= end)
    }

    fn resolve_grant(
        &self,
        key: &EntitlementKey,
        at: DateTime<Utc>,
        stale_after: Duration,
    ) -> EntitlementDecision<&EntitlementGrant> {
        if self.is_stale(at, stale_after) {
            return EntitlementDecision::StaleUntrusted;
        }
        let Some(grant) = self.grants.get(key) else {
            return EntitlementDecision::Unknown;
        };
        if grant.effective_start.is_some_and(|start| at < start)
            || grant.effective_end.is_some_and(|end| at >= end)
            || !grant.enabled
        {
            return EntitlementDecision::Denied;
        }
        EntitlementDecision::Granted(grant)
    }
}

/// Stable, extensible usage dimension key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct UsageDimension(String);

impl UsageDimension {
    /// Construct a bounded machine usage dimension.
    pub fn new(value: impl Into<String>) -> Result<Self, SaasDomainError> {
        Ok(Self(validate_machine_key(
            value.into(),
            "usage_dimension",
            128,
        )?))
    }

    /// Borrow the stable dimension representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UsageDimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for UsageDimension {
    type Err = SaasDomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value.to_owned())
    }
}

impl<'de> Deserialize<'de> for UsageDimension {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Source category for an immutable usage event.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// Event originated from the local application.
    Local,
    /// Event originated from a remote authority.
    Remote,
    /// Event was produced by reconciliation between sources.
    Reconciled,
}

impl UsageSource {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
            Self::Reconciled => "reconciled",
        }
    }
}

/// Ledger direction for a non-negative usage quantity.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageEventKind {
    /// Normal consumption that increases net usage.
    Consumption,
    /// Auditable compensating credit that reduces net usage.
    AdjustmentCredit,
}

impl UsageEventKind {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Consumption => "consumption",
            Self::AdjustmentCredit => "adjustment_credit",
        }
    }
}

/// Bounded scalar value accepted in usage metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UsageMetadataValue {
    /// Boolean machine flag.
    Bool(bool),
    /// Signed bounded integer.
    Integer(i64),
    /// Bounded machine string.
    String(String),
}

/// Bounded structured usage metadata that excludes arbitrary nested payloads.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct UsageMetadata(BTreeMap<String, UsageMetadataValue>);

impl<'de> Deserialize<'de> for UsageMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let values = BTreeMap::<String, UsageMetadataValue>::deserialize(deserializer)?;
        Self::new(values).map_err(serde::de::Error::custom)
    }
}

impl UsageMetadata {
    /// Validate and construct bounded structured metadata.
    pub fn new(
        values: BTreeMap<String, UsageMetadataValue>,
    ) -> Result<Self, SaasDomainError> {
        if values.len() > MAX_USAGE_METADATA_ENTRIES {
            return Err(SaasDomainError::UnsafeUsageMetadata("too many entries"));
        }
        for (key, value) in &values {
            validate_metadata_key(key)?;
            if let UsageMetadataValue::String(value) = value {
                if value.len() > MAX_USAGE_METADATA_STRING_BYTES {
                    return Err(SaasDomainError::UnsafeUsageMetadata(
                        "string value exceeds size bound",
                    ));
                }
                if !value.bytes().all(|b| {
                    b.is_ascii_alphanumeric()
                        || matches!(b, b'_' | b'-' | b'.' | b':')
                }) {
                    return Err(SaasDomainError::UnsafeUsageMetadata(
                        "string values must be bounded machine tokens, not free-form text",
                    ));
                }
            }
        }
        let encoded = serde_json::to_vec(&values)
            .map_err(|_| SaasDomainError::UnsafeUsageMetadata("serialization failed"))?;
        if encoded.len() > MAX_USAGE_METADATA_JSON_BYTES {
            return Err(SaasDomainError::UnsafeUsageMetadata(
                "serialized object exceeds size bound",
            ));
        }
        Ok(Self(values))
    }

    /// Borrow the structured metadata map.
    pub fn values(&self) -> &BTreeMap<String, UsageMetadataValue> {
        &self.0
    }

    /// Serialize to bounded JSON for persistence.
    pub fn to_json(&self) -> Result<String, SaasDomainError> {
        let encoded = serde_json::to_string(&self.0)
            .map_err(|_| SaasDomainError::UnsafeUsageMetadata("serialization failed"))?;
        if encoded.len() > MAX_USAGE_METADATA_JSON_BYTES {
            return Err(SaasDomainError::UnsafeUsageMetadata(
                "serialized object exceeds size bound",
            ));
        }
        Ok(encoded)
    }
}

fn validate_metadata_key(key: &str) -> Result<(), SaasDomainError> {
    if key.is_empty() || key.len() > MAX_USAGE_METADATA_KEY_BYTES {
        return Err(SaasDomainError::UnsafeUsageMetadata("invalid key length"));
    }
    if !key
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(SaasDomainError::UnsafeUsageMetadata(
            "keys must be machine-safe ASCII",
        ));
    }
    let lower = key.to_ascii_lowercase();
    const FORBIDDEN: &[&str] = &[
        "prompt",
        "document",
        "content",
        "message",
        "text",
        "query",
        "input",
        "output",
        "api_key",
        "secret",
        "token",
        "filesystem",
        "file_path",
        "path",
        "url",
        "uri",
        "email",
        "raw_error",
        "stacktrace",
        "stack_trace",
    ];
    if FORBIDDEN.iter().any(|needle| lower.contains(needle)) {
        return Err(SaasDomainError::UnsafeUsageMetadata(
            "key category is not permitted",
        ));
    }
    Ok(())
}

/// One immutable append-only usage ledger event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageEvent {
    /// Usage event business identity.
    pub usage_event_id: UsageEventId,
    /// Tenant billed/contracted for the usage.
    pub tenant_id: TenantId,
    /// Optional workspace ownership boundary.
    pub workspace_id: Option<WorkspaceId>,
    /// Optional actor that performed the action.
    pub actor_id: Option<ActorId>,
    /// Extensible usage dimension.
    pub dimension: UsageDimension,
    /// Non-negative quantity.
    pub quantity: u64,
    /// Whether this is consumption or a compensating credit.
    pub kind: UsageEventKind,
    /// When the usage actually occurred.
    pub occurred_at: DateTime<Utc>,
    /// When the event was durably recorded.
    pub recorded_at: DateTime<Utc>,
    /// Optional durable logical operation identity.
    pub operation_id: Option<OperationId>,
    /// Optional cross-component correlation identity.
    pub correlation_id: Option<CorrelationId>,
    /// Required replay-protection key.
    pub idempotency_key: IdempotencyKey,
    /// Origin category.
    pub source: UsageSource,
    /// Original event corrected by an adjustment credit.
    pub adjusts_usage_event_id: Option<UsageEventId>,
    /// Safe bounded structured metadata.
    pub metadata: UsageMetadata,
}

impl UsageEvent {
    /// Validate correction-shape invariants before persistence.
    pub fn validate(&self) -> Result<(), SaasDomainError> {
        match (self.kind, self.adjusts_usage_event_id.as_ref()) {
            (UsageEventKind::Consumption, None)
            | (UsageEventKind::AdjustmentCredit, Some(_)) => Ok(()),
            (UsageEventKind::Consumption, Some(_)) => Err(SaasDomainError::InvalidMachineKey {
                kind: "usage_adjustment",
                reason: "consumption must not reference a corrected event",
            }),
            (UsageEventKind::AdjustmentCredit, None) => Err(SaasDomainError::InvalidMachineKey {
                kind: "usage_adjustment",
                reason: "adjustment credit must reference the original event",
            }),
        }
    }
}

/// Aggregated debit/credit totals for a usage window.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageAggregation {
    /// Total consumption events.
    pub consumed: u64,
    /// Total compensating credits.
    pub credited: u64,
}

impl UsageAggregation {
    /// Resolve net usage, rejecting credit underflow instead of saturating.
    pub fn net(self) -> Result<u64, SaasDomainError> {
        self.consumed
            .checked_sub(self.credited)
            .ok_or(SaasDomainError::UsageUnderflow)
    }
}

/// Time-window policy independent of billing providers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UsageWindow {
    /// All recorded usage over the lifetime of the tenant/scope.
    Lifetime,
    /// UTC calendar day containing the decision timestamp.
    CalendarDay,
    /// UTC calendar month containing the decision timestamp.
    CalendarMonth,
    /// Fixed rolling window ending at the decision timestamp.
    Rolling {
        /// Positive rolling-window size in seconds.
        seconds: u64,
    },
    /// Externally supplied fixed billing-period boundaries.
    ExternalPeriod {
        /// Inclusive UTC period start.
        start: DateTime<Utc>,
        /// Exclusive UTC period end.
        end: DateTime<Utc>,
    },
}

/// Resolved UTC boundaries for a usage aggregation query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageWindowBounds {
    /// Inclusive start; `None` means unbounded past.
    pub start: Option<DateTime<Utc>>,
    /// Exclusive end; `None` means unbounded future/current lifetime query.
    pub end: Option<DateTime<Utc>>,
}

impl UsageWindow {
    /// Resolve the logical window into UTC query boundaries.
    pub fn bounds_at(&self, at: DateTime<Utc>) -> Result<UsageWindowBounds, SaasDomainError> {
        match self {
            Self::Lifetime => Ok(UsageWindowBounds {
                start: None,
                end: None,
            }),
            Self::CalendarDay => {
                let start = Utc
                    .with_ymd_and_hms(at.year(), at.month(), at.day(), 0, 0, 0)
                    .single()
                    .ok_or(SaasDomainError::InvalidUsageWindow("invalid UTC day"))?;
                let end = start
                    .checked_add_signed(Duration::days(1))
                    .ok_or(SaasDomainError::InvalidUsageWindow("UTC day end overflow"))?;
                Ok(UsageWindowBounds {
                    start: Some(start),
                    end: Some(end),
                })
            }
            Self::CalendarMonth => {
                let start = Utc
                    .with_ymd_and_hms(at.year(), at.month(), 1, 0, 0, 0)
                    .single()
                    .ok_or(SaasDomainError::InvalidUsageWindow("invalid UTC month"))?;
                let (next_year, next_month) = if at.month() == 12 {
                    (at.year() + 1, 1)
                } else {
                    (at.year(), at.month() + 1)
                };
                let end = Utc
                    .with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
                    .single()
                    .ok_or(SaasDomainError::InvalidUsageWindow("invalid UTC month end"))?;
                Ok(UsageWindowBounds {
                    start: Some(start),
                    end: Some(end),
                })
            }
            Self::Rolling { seconds } => {
                if *seconds == 0 || *seconds > i64::MAX as u64 {
                    return Err(SaasDomainError::InvalidUsageWindow(
                        "rolling seconds must be positive and bounded",
                    ));
                }
                let start = at
                    .checked_sub_signed(Duration::seconds(*seconds as i64))
                    .ok_or(SaasDomainError::InvalidUsageWindow(
                        "rolling window start overflow",
                    ))?;
                Ok(UsageWindowBounds {
                    start: Some(start),
                    end: Some(at),
                })
            }
            Self::ExternalPeriod { start, end } => {
                if end <= start {
                    return Err(SaasDomainError::InvalidUsageWindow(
                        "external period end must be after start",
                    ));
                }
                Ok(UsageWindowBounds {
                    start: Some(*start),
                    end: Some(*end),
                })
            }
        }
    }
}

/// Hard- versus soft-limit behavior.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaLimitBehavior {
    /// Exceeding the effective limit denies the operation.
    Hard,
    /// Exceeding the effective limit permits the operation with a warning.
    Soft,
}

impl QuotaLimitBehavior {
    /// Return the stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hard => "hard",
            Self::Soft => "soft",
        }
    }
}

/// Versioned quota policy for one tenant/workspace usage dimension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaPolicy {
    /// Policy business identity.
    pub policy_id: QuotaPolicyId,
    /// Tenant whose quota is governed.
    pub tenant_id: TenantId,
    /// Optional workspace-specific scope.
    pub workspace_id: Option<WorkspaceId>,
    /// Usage dimension governed by the policy.
    pub dimension: UsageDimension,
    /// Non-negative base limit.
    pub limit: u64,
    /// Aggregation window definition.
    pub window: UsageWindow,
    /// Hard or soft over-limit behavior.
    pub behavior: QuotaLimitBehavior,
    /// Optional extra allowance added to the base limit.
    pub burst_allowance: Option<u64>,
    /// Optional absolute usage threshold that triggers a warning.
    pub warning_threshold: Option<u64>,
    /// Optional entitlement required before this quota is considered.
    pub required_entitlement: Option<EntitlementKey>,
    /// Whether the policy is locally provisional or authoritative.
    pub authority: SnapshotAuthority,
    /// Origin of the policy record.
    pub source_of_truth: SourceOfTruth,
    /// Optional monotonic remote revision.
    pub source_revision: Option<RemoteRevision>,
    /// Optional stale marker.
    pub stale_marked_at: Option<DateTime<Utc>>,
    /// Creation timestamp in UTC.
    pub created_at: DateTime<Utc>,
}

/// Machine-readable quota decision state.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaDecisionState {
    /// Operation is permitted without a warning.
    Allowed,
    /// Operation is permitted but should surface a warning in a later layer.
    AllowedWithWarning,
    /// Operation is denied because the effective usage limit is exceeded.
    DeniedLimitExceeded,
    /// Operation is denied because a required entitlement is unavailable.
    DeniedEntitlementMissing,
    /// Operation is denied because subscription state is restrictive.
    DeniedSubscriptionRestricted,
    /// Decision cannot be trusted because authoritative state is stale.
    UnknownAuthoritativeStateStale,
}

/// Stable machine reason for a quota decision.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaDecisionReason {
    /// Usage remains below all thresholds.
    WithinLimit,
    /// Warning threshold was reached.
    WarningThresholdReached,
    /// Soft limit was exceeded but the operation remains allowed.
    SoftLimitExceeded,
    /// Hard limit was exceeded.
    HardLimitExceeded,
    /// Required entitlement was explicitly denied.
    EntitlementDenied,
    /// Required entitlement was not present.
    EntitlementUnknown,
    /// Subscription access is restricted.
    SubscriptionRestricted,
    /// One or more authoritative inputs are stale or untrusted.
    AuthoritativeStateStale,
}

/// Inputs to the pure quota decision function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaDecisionInput {
    /// Net usage already recorded in the resolved window.
    pub current_usage: u64,
    /// Non-negative quantity requested by the candidate operation.
    pub requested_quantity: u64,
    /// Required entitlement decision when the policy has an entitlement gate.
    pub entitlement: Option<EntitlementDecision<()>>,
    /// Resolved provider-neutral subscription access state.
    pub subscription: SubscriptionAccessState,
}

/// Pure quota decision result with machine-readable figures and reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaDecision {
    /// Final decision state.
    pub state: QuotaDecisionState,
    /// Stable machine reason.
    pub reason: QuotaDecisionReason,
    /// Usage before the candidate operation.
    pub current_usage: u64,
    /// Candidate quantity.
    pub requested_quantity: u64,
    /// Usage after the candidate operation when arithmetic is valid.
    pub projected_usage: u64,
    /// Base configured limit.
    pub limit: u64,
    /// Effective limit after optional burst allowance.
    pub effective_limit: u64,
    /// Optional configured warning threshold.
    pub warning_threshold: Option<u64>,
}

impl QuotaPolicy {
    /// Evaluate a deterministic quota decision without I/O or localized text.
    pub fn decide(&self, input: QuotaDecisionInput) -> Result<QuotaDecision, SaasDomainError> {
        let projected = input
            .current_usage
            .checked_add(input.requested_quantity)
            .ok_or(SaasDomainError::UsageOverflow)?;
        let effective_limit = self
            .limit
            .checked_add(self.burst_allowance.unwrap_or(0))
            .ok_or(SaasDomainError::UsageOverflow)?;
        let base = |state, reason| QuotaDecision {
            state,
            reason,
            current_usage: input.current_usage,
            requested_quantity: input.requested_quantity,
            projected_usage: projected,
            limit: self.limit,
            effective_limit,
            warning_threshold: self.warning_threshold,
        };

        if self.authority != SnapshotAuthority::Authoritative || self.stale_marked_at.is_some() {
            return Ok(base(
                QuotaDecisionState::UnknownAuthoritativeStateStale,
                QuotaDecisionReason::AuthoritativeStateStale,
            ));
        }

        match input.subscription {
            SubscriptionAccessState::StaleOrUnknown => {
                return Ok(base(
                    QuotaDecisionState::UnknownAuthoritativeStateStale,
                    QuotaDecisionReason::AuthoritativeStateStale,
                ));
            }
            SubscriptionAccessState::Restricted => {
                return Ok(base(
                    QuotaDecisionState::DeniedSubscriptionRestricted,
                    QuotaDecisionReason::SubscriptionRestricted,
                ));
            }
            SubscriptionAccessState::Allowed => {}
        }

        if self.required_entitlement.is_some() {
            match input.entitlement {
                Some(EntitlementDecision::Granted(())) => {}
                Some(EntitlementDecision::StaleUntrusted) => {
                    return Ok(base(
                        QuotaDecisionState::UnknownAuthoritativeStateStale,
                        QuotaDecisionReason::AuthoritativeStateStale,
                    ));
                }
                Some(EntitlementDecision::Denied) => {
                    return Ok(base(
                        QuotaDecisionState::DeniedEntitlementMissing,
                        QuotaDecisionReason::EntitlementDenied,
                    ));
                }
                Some(EntitlementDecision::Unknown) | None => {
                    return Ok(base(
                        QuotaDecisionState::DeniedEntitlementMissing,
                        QuotaDecisionReason::EntitlementUnknown,
                    ));
                }
            }
        }

        if projected > effective_limit {
            return Ok(match self.behavior {
                QuotaLimitBehavior::Hard => base(
                    QuotaDecisionState::DeniedLimitExceeded,
                    QuotaDecisionReason::HardLimitExceeded,
                ),
                QuotaLimitBehavior::Soft => base(
                    QuotaDecisionState::AllowedWithWarning,
                    QuotaDecisionReason::SoftLimitExceeded,
                ),
            });
        }

        if self
            .warning_threshold
            .is_some_and(|threshold| projected >= threshold)
        {
            return Ok(base(
                QuotaDecisionState::AllowedWithWarning,
                QuotaDecisionReason::WarningThresholdReached,
            ));
        }

        Ok(base(
            QuotaDecisionState::Allowed,
            QuotaDecisionReason::WithinLimit,
        ))
    }
}

fn validate_machine_key(
    value: String,
    kind: &'static str,
    max_bytes: usize,
) -> Result<String, SaasDomainError> {
    if value.trim().is_empty() {
        return Err(SaasDomainError::InvalidMachineKey {
            kind,
            reason: "empty",
        });
    }
    if value.len() > max_bytes {
        return Err(SaasDomainError::InvalidMachineKey {
            kind,
            reason: "too_long",
        });
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':' | b'/'))
    {
        return Err(SaasDomainError::InvalidMachineKey {
            kind,
            reason: "invalid_characters",
        });
    }
    Ok(value)
}
