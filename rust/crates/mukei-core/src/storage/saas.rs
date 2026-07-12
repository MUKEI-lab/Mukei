//! Persistence repositories for the local-first SaaS domain.
//!
//! The repository layer keeps SQLite transactions and revision/idempotency
//! invariants out of UI, bridge, agent, and transport code.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, TransactionBehavior};

use crate::error::{MukeiError, Result};
use crate::saas::{
    Actor, ActorId, ActorKind, ActorStatus, CorrelationId, EntitlementGrant, EntitlementKey,
    EntitlementSnapshot, EntitlementSnapshotId, IdempotencyKey, LocalScope, MembershipId,
    MembershipRole, MembershipStatus, OperationId, QuotaLimitBehavior, QuotaPolicy, QuotaPolicyId,
    RemoteRevision, SnapshotAuthority, SourceOfTruth, SubscriptionSnapshotId, SubscriptionState,
    SubscriptionStatus, Tenant, TenantId, TenantStatus, UsageAggregation, UsageDimension, UsageEvent,
    UsageEventId, UsageEventKind, UsageMetadata, UsageSource, UsageWindow, Workspace,
    WorkspaceId, WorkspaceManagement, WorkspaceMembership, WorkspaceStatus,
};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};

/// Outcome of applying a revisioned durable record.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RecordApplyOutcome {
    /// Incoming record was inserted or replaced an older version.
    Applied,
    /// Incoming record exactly matched the already-applied revision.
    Reapplied,
    /// Incoming record was older than the durable current record.
    IgnoredOlder,
}

/// Outcome of applying an immutable versioned snapshot.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SnapshotApplyOutcome {
    /// Incoming snapshot became current.
    Applied,
    /// The same immutable snapshot was already current.
    Reapplied,
    /// Incoming snapshot was older than the current snapshot.
    IgnoredOlder,
}

/// Outcome of appending an idempotent usage mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsageAppendOutcome {
    /// A new immutable ledger event was appended.
    Inserted(UsageEvent),
    /// An equivalent logical mutation already existed and was returned.
    Existing(UsageEvent),
}

/// Repository for tenants, workspaces, and deterministic local scope bootstrap.
pub struct TenantWorkspaceRepository;

impl TenantWorkspaceRepository {
    /// Ensure deterministic local tenant/workspace/actor rows exist without
    /// changing any existing conversation or document ownership schema.
    pub async fn ensure_local_scope(
        pool: &DatabasePool,
        installation_id: impl Into<String>,
    ) -> Result<LocalScope> {
        let installation_id = installation_id.into();
        let scope = LocalScope::for_installation(&installation_id).map_err(domain_error)?;
        let now = Utc::now();
        let tenant = scope.tenant(now);
        let workspace = scope.workspace(now);
        let actor = scope.actor(now);
        let membership = scope.membership(now);
        let scope_out = scope.clone();
        pool.with_conn(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            insert_local_tenant_if_missing(&tx, &tenant)?;
            insert_local_workspace_if_missing(&tx, &workspace)?;
            insert_local_actor_if_missing(&tx, &actor)?;
            insert_local_membership_if_missing(&tx, &membership)?;
            tx.commit()?;
            Ok::<_, DbError>(scope_out)
        })
        .await
    }

    /// Insert/update a local tenant without allowing local state to overwrite a
    /// remotely authoritative tenant record.
    pub async fn upsert_local_tenant(pool: &DatabasePool, tenant: Tenant) -> Result<()> {
        validate_tenant(&tenant)?;
        if tenant.source_of_truth != SourceOfTruth::Local {
            return Err(MukeiError::Invariant(
                "upsert_local_tenant requires a local source".into(),
            ));
        }
        pool.with_conn(move |c| {
            c.execute(
                "INSERT INTO saas_tenants (tenant_id, display_name, status, source_of_truth, \
                    created_at, updated_at, remote_revision, suspension_reason) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7) \
                 ON CONFLICT(tenant_id) DO UPDATE SET \
                    display_name = excluded.display_name, status = excluded.status, \
                    updated_at = excluded.updated_at, suspension_reason = excluded.suspension_reason \
                 WHERE saas_tenants.source_of_truth = 'local'",
                rusqlite::params![
                    tenant.tenant_id.as_str(),
                    tenant.display_name,
                    tenant.status.as_str(),
                    tenant.source_of_truth.as_str(),
                    tenant.created_at.to_rfc3339(),
                    tenant.updated_at.to_rfc3339(),
                    tenant.suspension_reason,
                ],
            )?;
            Ok::<_, DbError>(())
        })
        .await
    }

    /// Apply a remotely authoritative tenant using monotonic revision ordering.
    pub async fn apply_remote_tenant(
        pool: &DatabasePool,
        tenant: Tenant,
    ) -> Result<RecordApplyOutcome> {
        validate_tenant(&tenant)?;
        if tenant.source_of_truth != SourceOfTruth::Remote || tenant.remote_revision.is_none() {
            return Err(MukeiError::Invariant(
                "remote tenant application requires remote source and revision".into(),
            ));
        }
        pool.with_conn(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = load_tenant_conn(&tx, &tenant.tenant_id)?;
            if let Some(existing) = existing {
                match revision_order(existing.remote_revision, tenant.remote_revision) {
                    RevisionOrder::IncomingOlder => {
                        tx.commit()?;
                        return Ok(RecordApplyOutcome::IgnoredOlder);
                    }
                    RevisionOrder::Same => {
                        if existing == tenant {
                            tx.commit()?;
                            return Ok(RecordApplyOutcome::Reapplied);
                        }
                        return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                            entity: "tenant",
                            revision: tenant.remote_revision.unwrap_or(0),
                        }));
                    }
                    RevisionOrder::IncomingNewer => {}
                }
            }
            tx.execute(
                "INSERT INTO saas_tenants (tenant_id, display_name, status, source_of_truth, \
                    created_at, updated_at, remote_revision, suspension_reason) \
                 VALUES (?1, ?2, ?3, 'remote', ?4, ?5, ?6, ?7) \
                 ON CONFLICT(tenant_id) DO UPDATE SET \
                    display_name = excluded.display_name, status = excluded.status, \
                    source_of_truth = 'remote', created_at = excluded.created_at, \
                    updated_at = excluded.updated_at, remote_revision = excluded.remote_revision, \
                    suspension_reason = excluded.suspension_reason",
                rusqlite::params![
                    tenant.tenant_id.as_str(),
                    tenant.display_name,
                    tenant.status.as_str(),
                    tenant.created_at.to_rfc3339(),
                    tenant.updated_at.to_rfc3339(),
                    to_sql_u64(tenant.remote_revision.unwrap(), "tenant.remote_revision")?,
                    tenant.suspension_reason,
                ],
            )?;
            tx.commit()?;
            Ok(RecordApplyOutcome::Applied)
        })
        .await
    }

    /// Load one tenant by opaque business identity.
    pub async fn get_tenant(pool: &DatabasePool, tenant_id: TenantId) -> Result<Option<Tenant>> {
        pool.with_conn(move |c| load_tenant_conn(c, &tenant_id)).await
    }

    /// Insert/update a local workspace without overwriting a remotely managed
    /// workspace record.
    pub async fn upsert_local_workspace(pool: &DatabasePool, workspace: Workspace) -> Result<()> {
        validate_workspace(&workspace)?;
        if workspace.management != WorkspaceManagement::LocalOnly {
            return Err(MukeiError::Invariant(
                "upsert_local_workspace requires local-only management".into(),
            ));
        }
        pool.with_conn(move |c| {
            c.execute(
                "INSERT INTO saas_workspaces (workspace_id, tenant_id, display_name, status, \
                    management_mode, created_at, updated_at, remote_revision) \
                 VALUES (?1, ?2, ?3, ?4, 'local_only', ?5, ?6, NULL) \
                 ON CONFLICT(workspace_id) DO UPDATE SET \
                    display_name = excluded.display_name, status = excluded.status, \
                    updated_at = excluded.updated_at \
                 WHERE saas_workspaces.management_mode = 'local_only'",
                rusqlite::params![
                    workspace.workspace_id.as_str(),
                    workspace.tenant_id.as_str(),
                    workspace.display_name,
                    workspace.status.as_str(),
                    workspace.created_at.to_rfc3339(),
                    workspace.updated_at.to_rfc3339(),
                ],
            )?;
            Ok::<_, DbError>(())
        })
        .await
    }

    /// Apply a remotely managed workspace using monotonic revision ordering.
    pub async fn apply_remote_workspace(
        pool: &DatabasePool,
        workspace: Workspace,
    ) -> Result<RecordApplyOutcome> {
        validate_workspace(&workspace)?;
        if workspace.management != WorkspaceManagement::RemoteManaged
            || workspace.remote_revision.is_none()
        {
            return Err(MukeiError::Invariant(
                "remote workspace application requires remote management and revision".into(),
            ));
        }
        pool.with_conn(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = load_workspace_conn(&tx, &workspace.workspace_id)?;
            if let Some(existing) = existing {
                if existing.tenant_id != workspace.tenant_id {
                    return Err(DbError::Domain(MukeiError::Invariant(
                        "workspace tenant ownership is immutable".into(),
                    )));
                }
                match revision_order(existing.remote_revision, workspace.remote_revision) {
                    RevisionOrder::IncomingOlder => {
                        tx.commit()?;
                        return Ok(RecordApplyOutcome::IgnoredOlder);
                    }
                    RevisionOrder::Same => {
                        if existing == workspace {
                            tx.commit()?;
                            return Ok(RecordApplyOutcome::Reapplied);
                        }
                        return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                            entity: "workspace",
                            revision: workspace.remote_revision.unwrap_or(0),
                        }));
                    }
                    RevisionOrder::IncomingNewer => {}
                }
            }
            tx.execute(
                "INSERT INTO saas_workspaces (workspace_id, tenant_id, display_name, status, \
                    management_mode, created_at, updated_at, remote_revision) \
                 VALUES (?1, ?2, ?3, ?4, 'remote_managed', ?5, ?6, ?7) \
                 ON CONFLICT(workspace_id) DO UPDATE SET \
                    tenant_id = excluded.tenant_id, display_name = excluded.display_name, \
                    status = excluded.status, management_mode = 'remote_managed', \
                    created_at = excluded.created_at, updated_at = excluded.updated_at, \
                    remote_revision = excluded.remote_revision",
                rusqlite::params![
                    workspace.workspace_id.as_str(),
                    workspace.tenant_id.as_str(),
                    workspace.display_name,
                    workspace.status.as_str(),
                    workspace.created_at.to_rfc3339(),
                    workspace.updated_at.to_rfc3339(),
                    to_sql_u64(workspace.remote_revision.unwrap(), "workspace.remote_revision")?,
                ],
            )?;
            tx.commit()?;
            Ok(RecordApplyOutcome::Applied)
        })
        .await
    }

    /// Load one workspace by opaque business identity.
    pub async fn get_workspace(
        pool: &DatabasePool,
        workspace_id: WorkspaceId,
    ) -> Result<Option<Workspace>> {
        pool.with_conn(move |c| load_workspace_conn(c, &workspace_id))
            .await
    }
}

/// Repository for actors and workspace memberships.
pub struct MembershipRepository;

impl MembershipRepository {
    /// Upsert an installation-local actor without overwriting a remote actor.
    pub async fn upsert_local_actor(pool: &DatabasePool, actor: Actor) -> Result<()> {
        validate_actor(&actor)?;
        if actor.source_of_truth != SourceOfTruth::Local {
            return Err(MukeiError::Invariant(
                "upsert_local_actor requires a local source".into(),
            ));
        }
        pool.with_conn(move |c| {
            insert_or_update_local_actor(c, &actor)?;
            Ok::<_, DbError>(())
        })
        .await
    }

    /// Apply a remote actor using monotonic revision ordering.
    pub async fn apply_remote_actor(
        pool: &DatabasePool,
        actor: Actor,
    ) -> Result<RecordApplyOutcome> {
        validate_actor(&actor)?;
        if actor.source_of_truth != SourceOfTruth::Remote || actor.remote_revision.is_none() {
            return Err(MukeiError::Invariant(
                "remote actor application requires remote source and revision".into(),
            ));
        }
        pool.with_conn(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = load_actor_conn(&tx, &actor.actor_id)?;
            if let Some(existing) = existing {
                match revision_order(existing.remote_revision, actor.remote_revision) {
                    RevisionOrder::IncomingOlder => {
                        tx.commit()?;
                        return Ok(RecordApplyOutcome::IgnoredOlder);
                    }
                    RevisionOrder::Same => {
                        if existing == actor {
                            tx.commit()?;
                            return Ok(RecordApplyOutcome::Reapplied);
                        }
                        return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                            entity: "actor",
                            revision: actor.remote_revision.unwrap_or(0),
                        }));
                    }
                    RevisionOrder::IncomingNewer => {}
                }
            }
            tx.execute(
                "INSERT INTO saas_actors (actor_id, display_name, kind, status, source_of_truth, \
                    created_at, updated_at, remote_revision) \
                 VALUES (?1, ?2, ?3, ?4, 'remote', ?5, ?6, ?7) \
                 ON CONFLICT(actor_id) DO UPDATE SET \
                    display_name = excluded.display_name, kind = excluded.kind, \
                    status = excluded.status, source_of_truth = 'remote', \
                    created_at = excluded.created_at, updated_at = excluded.updated_at, \
                    remote_revision = excluded.remote_revision",
                rusqlite::params![
                    actor.actor_id.as_str(),
                    actor.display_name,
                    actor.kind.as_str(),
                    actor.status.as_str(),
                    actor.created_at.to_rfc3339(),
                    actor.updated_at.to_rfc3339(),
                    to_sql_u64(actor.remote_revision.unwrap(), "actor.remote_revision")?,
                ],
            )?;
            tx.commit()?;
            Ok(RecordApplyOutcome::Applied)
        })
        .await
    }

    /// Load one actor by opaque business identity.
    pub async fn get_actor(pool: &DatabasePool, actor_id: ActorId) -> Result<Option<Actor>> {
        pool.with_conn(move |c| load_actor_conn(c, &actor_id)).await
    }

    /// Upsert a local membership without overwriting a revisioned remote row.
    pub async fn upsert_local_membership(
        pool: &DatabasePool,
        membership: WorkspaceMembership,
    ) -> Result<()> {
        validate_membership(&membership)?;
        if membership.remote_revision.is_some() {
            return Err(MukeiError::Invariant(
                "local membership must not carry a remote revision".into(),
            ));
        }
        pool.with_conn(move |c| {
            c.execute(
                "INSERT INTO saas_workspace_memberships (membership_id, tenant_id, workspace_id, \
                    actor_id, role, status, created_at, updated_at, remote_revision) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL) \
                 ON CONFLICT(membership_id) DO UPDATE SET \
                    role = excluded.role, status = excluded.status, updated_at = excluded.updated_at \
                 WHERE saas_workspace_memberships.remote_revision IS NULL",
                rusqlite::params![
                    membership.membership_id.as_str(),
                    membership.tenant_id.as_str(),
                    membership.workspace_id.as_str(),
                    membership.actor_id.as_str(),
                    membership.role.as_str(),
                    membership.status.as_str(),
                    membership.created_at.to_rfc3339(),
                    membership.updated_at.to_rfc3339(),
                ],
            )?;
            Ok::<_, DbError>(())
        })
        .await
    }

    /// Apply a remote workspace membership using monotonic revision ordering.
    pub async fn apply_remote_membership(
        pool: &DatabasePool,
        membership: WorkspaceMembership,
    ) -> Result<RecordApplyOutcome> {
        validate_membership(&membership)?;
        if membership.remote_revision.is_none() {
            return Err(MukeiError::Invariant(
                "remote membership application requires a revision".into(),
            ));
        }
        pool.with_conn(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = load_membership_conn(&tx, &membership.membership_id)?;
            if let Some(existing) = existing {
                if existing.tenant_id != membership.tenant_id
                    || existing.workspace_id != membership.workspace_id
                    || existing.actor_id != membership.actor_id
                {
                    return Err(DbError::Domain(MukeiError::Invariant(
                        "membership ownership bindings are immutable".into(),
                    )));
                }
                match revision_order(existing.remote_revision, membership.remote_revision) {
                    RevisionOrder::IncomingOlder => {
                        tx.commit()?;
                        return Ok(RecordApplyOutcome::IgnoredOlder);
                    }
                    RevisionOrder::Same => {
                        if existing == membership {
                            tx.commit()?;
                            return Ok(RecordApplyOutcome::Reapplied);
                        }
                        return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                            entity: "membership",
                            revision: membership.remote_revision.unwrap_or(0),
                        }));
                    }
                    RevisionOrder::IncomingNewer => {}
                }
            }
            tx.execute(
                "INSERT INTO saas_workspace_memberships (membership_id, tenant_id, workspace_id, \
                    actor_id, role, status, created_at, updated_at, remote_revision) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
                 ON CONFLICT(membership_id) DO UPDATE SET \
                    tenant_id = excluded.tenant_id, workspace_id = excluded.workspace_id, \
                    actor_id = excluded.actor_id, role = excluded.role, status = excluded.status, \
                    created_at = excluded.created_at, updated_at = excluded.updated_at, \
                    remote_revision = excluded.remote_revision",
                rusqlite::params![
                    membership.membership_id.as_str(),
                    membership.tenant_id.as_str(),
                    membership.workspace_id.as_str(),
                    membership.actor_id.as_str(),
                    membership.role.as_str(),
                    membership.status.as_str(),
                    membership.created_at.to_rfc3339(),
                    membership.updated_at.to_rfc3339(),
                    to_sql_u64(
                        membership.remote_revision.unwrap(),
                        "membership.remote_revision",
                    )?,
                ],
            )?;
            tx.commit()?;
            Ok(RecordApplyOutcome::Applied)
        })
        .await
    }

    /// Load one membership by opaque business identity.
    pub async fn get_membership(
        pool: &DatabasePool,
        membership_id: MembershipId,
    ) -> Result<Option<WorkspaceMembership>> {
        pool.with_conn(move |c| load_membership_conn(c, &membership_id))
            .await
    }
}

/// Repository for immutable provider-neutral subscription snapshots.
pub struct SubscriptionRepository;

impl SubscriptionRepository {
    /// Apply a subscription snapshot transactionally and preserve older history.
    pub async fn apply_snapshot(
        pool: &DatabasePool,
        snapshot: SubscriptionState,
    ) -> Result<SnapshotApplyOutcome> {
        validate_subscription(&snapshot)?;
        pool.with_conn(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current = load_current_subscription_conn(&tx, &snapshot.tenant_id)?;
            if let Some(current) = current {
                match snapshot_order(
                    SnapshotIdentity {
                        id: current.snapshot_id.as_str(),
                        revision: current.source_revision,
                        authority: current.authority,
                        observed_at: current.last_synced_at,
                    },
                    SnapshotIdentity {
                        id: snapshot.snapshot_id.as_str(),
                        revision: snapshot.source_revision,
                        authority: snapshot.authority,
                        observed_at: snapshot.last_synced_at,
                    },
                ) {
                    SnapshotOrder::IncomingOlder => {
                        tx.commit()?;
                        return Ok(SnapshotApplyOutcome::IgnoredOlder);
                    }
                    SnapshotOrder::SameIdentity => {
                        if subscription_snapshot_equivalent(&current, &snapshot) {
                            tx.execute(
                                "UPDATE saas_subscription_snapshots SET stale_marked_at = ?2 \
                                 WHERE snapshot_id = ?1",
                                rusqlite::params![
                                    snapshot.snapshot_id.as_str(),
                                    snapshot.stale_marked_at.map(|v| v.to_rfc3339()),
                                ],
                            )?;
                            tx.commit()?;
                            return Ok(SnapshotApplyOutcome::Reapplied);
                        }
                        return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                            entity: "subscription",
                            revision: snapshot.source_revision.unwrap_or(0),
                        }));
                    }
                    SnapshotOrder::SameRevisionDifferentIdentity(revision) => {
                        return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                            entity: "subscription",
                            revision,
                        }));
                    }
                    SnapshotOrder::IncomingNewer => {}
                }
            }

            let existing_same_id = load_subscription_by_id_conn(&tx, &snapshot.snapshot_id)?;
            if let Some(existing) = existing_same_id {
                if subscription_snapshot_equivalent(&existing, &snapshot) {
                    tx.commit()?;
                    return Ok(SnapshotApplyOutcome::IgnoredOlder);
                }
                return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                    entity: "subscription",
                    revision: snapshot.source_revision.unwrap_or(0),
                }));
            }

            tx.execute(
                "UPDATE saas_subscription_snapshots SET is_current = 0 \
                 WHERE tenant_id = ?1 AND is_current = 1",
                [snapshot.tenant_id.as_str()],
            )?;
            tx.execute(
                "INSERT INTO saas_subscription_snapshots (snapshot_id, tenant_id, plan_key, status, \
                    effective_start, effective_end, grace_period_end, source_revision, \
                    last_synced_at, authority, stale_marked_at, is_current, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12)",
                rusqlite::params![
                    snapshot.snapshot_id.as_str(),
                    snapshot.tenant_id.as_str(),
                    snapshot.plan_key,
                    snapshot.status.as_str(),
                    snapshot.effective_start.to_rfc3339(),
                    snapshot.effective_end.map(|v| v.to_rfc3339()),
                    snapshot.grace_period_end.map(|v| v.to_rfc3339()),
                    opt_sql_u64(snapshot.source_revision, "subscription.source_revision")?,
                    snapshot.last_synced_at.to_rfc3339(),
                    snapshot.authority.as_str(),
                    snapshot.stale_marked_at.map(|v| v.to_rfc3339()),
                    Utc::now().to_rfc3339(),
                ],
            )?;
            tx.commit()?;
            Ok(SnapshotApplyOutcome::Applied)
        })
        .await
    }

    /// Resolve the current subscription snapshot for a tenant.
    pub async fn current(
        pool: &DatabasePool,
        tenant_id: TenantId,
    ) -> Result<Option<SubscriptionState>> {
        pool.with_conn(move |c| load_current_subscription_conn(c, &tenant_id))
            .await
    }

    /// Mark the current subscription stale without deleting history.
    pub async fn mark_current_stale(
        pool: &DatabasePool,
        tenant_id: TenantId,
        marked_at: DateTime<Utc>,
    ) -> Result<bool> {
        pool.with_conn(move |c| {
            let changed = c.execute(
                "UPDATE saas_subscription_snapshots SET stale_marked_at = ?2 \
                 WHERE tenant_id = ?1 AND is_current = 1",
                rusqlite::params![tenant_id.as_str(), marked_at.to_rfc3339()],
            )?;
            Ok::<_, DbError>(changed > 0)
        })
        .await
    }
}

/// Repository for immutable entitlement snapshots and grants.
pub struct EntitlementRepository;

impl EntitlementRepository {
    /// Apply an immutable entitlement snapshot and activate it transactionally.
    pub async fn apply_snapshot(
        pool: &DatabasePool,
        snapshot: EntitlementSnapshot,
    ) -> Result<SnapshotApplyOutcome> {
        validate_entitlement_snapshot(&snapshot)?;
        pool.with_conn(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current = load_current_entitlement_conn(&tx, &snapshot.tenant_id)?;
            if let Some(current) = current {
                match snapshot_order(
                    SnapshotIdentity {
                        id: current.snapshot_id.as_str(),
                        revision: current.source_revision,
                        authority: current.authority,
                        observed_at: current.last_synced_at,
                    },
                    SnapshotIdentity {
                        id: snapshot.snapshot_id.as_str(),
                        revision: snapshot.source_revision,
                        authority: snapshot.authority,
                        observed_at: snapshot.last_synced_at,
                    },
                ) {
                    SnapshotOrder::IncomingOlder => {
                        tx.commit()?;
                        return Ok(SnapshotApplyOutcome::IgnoredOlder);
                    }
                    SnapshotOrder::SameIdentity => {
                        if entitlement_snapshot_equivalent(&current, &snapshot) {
                            tx.execute(
                                "UPDATE saas_entitlement_snapshots SET stale_marked_at = ?2 \
                                 WHERE snapshot_id = ?1",
                                rusqlite::params![
                                    snapshot.snapshot_id.as_str(),
                                    snapshot.stale_marked_at.map(|v| v.to_rfc3339()),
                                ],
                            )?;
                            tx.commit()?;
                            return Ok(SnapshotApplyOutcome::Reapplied);
                        }
                        return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                            entity: "entitlement",
                            revision: snapshot.source_revision.unwrap_or(0),
                        }));
                    }
                    SnapshotOrder::SameRevisionDifferentIdentity(revision) => {
                        return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                            entity: "entitlement",
                            revision,
                        }));
                    }
                    SnapshotOrder::IncomingNewer => {}
                }
            }

            if let Some(existing) = load_entitlement_by_id_conn(&tx, &snapshot.snapshot_id)? {
                if entitlement_snapshot_equivalent(&existing, &snapshot) {
                    tx.commit()?;
                    return Ok(SnapshotApplyOutcome::IgnoredOlder);
                }
                return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                    entity: "entitlement",
                    revision: snapshot.source_revision.unwrap_or(0),
                }));
            }

            tx.execute(
                "UPDATE saas_entitlement_snapshots SET is_current = 0 \
                 WHERE tenant_id = ?1 AND is_current = 1",
                [snapshot.tenant_id.as_str()],
            )?;
            tx.execute(
                "INSERT INTO saas_entitlement_snapshots (snapshot_id, tenant_id, source_revision, \
                    last_synced_at, authority, stale_marked_at, is_current, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
                rusqlite::params![
                    snapshot.snapshot_id.as_str(),
                    snapshot.tenant_id.as_str(),
                    opt_sql_u64(snapshot.source_revision, "entitlement.source_revision")?,
                    snapshot.last_synced_at.to_rfc3339(),
                    snapshot.authority.as_str(),
                    snapshot.stale_marked_at.map(|v| v.to_rfc3339()),
                    Utc::now().to_rfc3339(),
                ],
            )?;
            for grant in snapshot.grants.values() {
                tx.execute(
                    "INSERT INTO saas_entitlement_grants (snapshot_id, entitlement_key, enabled, \
                        numeric_limit, string_value, effective_start, effective_end, source_revision) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        snapshot.snapshot_id.as_str(),
                        grant.key.as_str(),
                        if grant.enabled { 1_i64 } else { 0_i64 },
                        opt_sql_u64(grant.numeric_limit, "entitlement.numeric_limit")?,
                        grant.string_value,
                        grant.effective_start.map(|v| v.to_rfc3339()),
                        grant.effective_end.map(|v| v.to_rfc3339()),
                        opt_sql_u64(grant.source_revision, "entitlement.grant_revision")?,
                    ],
                )?;
            }
            tx.commit()?;
            Ok(SnapshotApplyOutcome::Applied)
        })
        .await
    }

    /// Resolve the single effective entitlement snapshot for a tenant.
    pub async fn current(
        pool: &DatabasePool,
        tenant_id: TenantId,
    ) -> Result<Option<EntitlementSnapshot>> {
        pool.with_conn(move |c| load_current_entitlement_conn(c, &tenant_id))
            .await
    }

    /// Mark the current entitlement snapshot stale without deleting it.
    pub async fn mark_current_stale(
        pool: &DatabasePool,
        tenant_id: TenantId,
        marked_at: DateTime<Utc>,
    ) -> Result<bool> {
        pool.with_conn(move |c| {
            let changed = c.execute(
                "UPDATE saas_entitlement_snapshots SET stale_marked_at = ?2 \
                 WHERE tenant_id = ?1 AND is_current = 1",
                rusqlite::params![tenant_id.as_str(), marked_at.to_rfc3339()],
            )?;
            Ok::<_, DbError>(changed > 0)
        })
        .await
    }
}

/// Repository for the immutable idempotent usage ledger.
pub struct UsageLedgerRepository;

impl UsageLedgerRepository {
    /// Append one logical usage mutation, returning the existing event for an
    /// equivalent replay and rejecting payload-changing key reuse.
    pub async fn append(pool: &DatabasePool, event: UsageEvent) -> Result<UsageAppendOutcome> {
        event.validate().map_err(domain_error)?;
        let _ = to_sql_u64(event.quantity, "usage.quantity")?;
        let _ = event.metadata.to_json().map_err(domain_error)?;
        pool.with_conn(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = load_usage_by_idempotency_conn(
                &tx,
                &event.tenant_id,
                &event.dimension,
                &event.idempotency_key,
            )?;
            if let Some(existing) = existing {
                if usage_mutation_equivalent(&existing, &event) {
                    tx.commit()?;
                    return Ok(UsageAppendOutcome::Existing(existing));
                }
                return Err(DbError::Domain(MukeiError::UsageIdempotencyConflict {
                    tenant_id: event.tenant_id.to_string(),
                    dimension: event.dimension.to_string(),
                }));
            }

            validate_adjustment_credit_conn(&tx, &event)?;
            let metadata_json = event.metadata.to_json().map_err(domain_db_error)?;
            tx.execute(
                "INSERT INTO saas_usage_events (usage_event_id, tenant_id, workspace_id, actor_id, \
                    dimension, quantity, event_kind, occurred_at, recorded_at, operation_id, \
                    correlation_id, idempotency_key, source, adjusts_usage_event_id, metadata_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![
                    event.usage_event_id.as_str(),
                    event.tenant_id.as_str(),
                    event.workspace_id.as_ref().map(WorkspaceId::as_str),
                    event.actor_id.as_ref().map(ActorId::as_str),
                    event.dimension.as_str(),
                    to_sql_u64(event.quantity, "usage.quantity")?,
                    event.kind.as_str(),
                    event.occurred_at.to_rfc3339(),
                    event.recorded_at.to_rfc3339(),
                    event.operation_id.as_ref().map(OperationId::as_str),
                    event.correlation_id.as_ref().map(CorrelationId::as_str),
                    event.idempotency_key.as_str(),
                    event.source.as_str(),
                    event.adjusts_usage_event_id.as_ref().map(UsageEventId::as_str),
                    metadata_json,
                ],
            )?;
            tx.commit()?;
            Ok(UsageAppendOutcome::Inserted(event))
        })
        .await
    }

    /// Load one immutable usage event by business identity.
    pub async fn get_event(
        pool: &DatabasePool,
        usage_event_id: UsageEventId,
    ) -> Result<Option<UsageEvent>> {
        pool.with_conn(move |c| load_usage_by_id_conn(c, &usage_event_id))
            .await
    }

    /// Aggregate consumption and compensating credits within resolved UTC
    /// boundaries. `workspace_id = None` aggregates tenant-wide usage.
    pub async fn aggregate(
        pool: &DatabasePool,
        tenant_id: TenantId,
        workspace_id: Option<WorkspaceId>,
        dimension: UsageDimension,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> Result<UsageAggregation> {
        pool.with_conn(move |c| {
            let workspace = workspace_id.as_ref().map(WorkspaceId::as_str);
            let start = start.map(|v| v.to_rfc3339());
            let end = end.map(|v| v.to_rfc3339());
            let (consumed, credited): (i64, i64) = c.query_row(
                "SELECT \
                    COALESCE(SUM(CASE WHEN event_kind = 'consumption' THEN quantity ELSE 0 END), 0), \
                    COALESCE(SUM(CASE WHEN event_kind = 'adjustment_credit' THEN quantity ELSE 0 END), 0) \
                 FROM saas_usage_events \
                 WHERE tenant_id = ?1 AND dimension = ?2 \
                   AND (?3 IS NULL OR workspace_id = ?3) \
                   AND (?4 IS NULL OR occurred_at >= ?4) \
                   AND (?5 IS NULL OR occurred_at < ?5)",
                rusqlite::params![tenant_id.as_str(), dimension.as_str(), workspace, start, end],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            Ok::<_, DbError>(UsageAggregation {
                consumed: from_sql_u64(consumed, "usage.aggregate.consumed")?,
                credited: from_sql_u64(credited, "usage.aggregate.credited")?,
            })
        })
        .await
    }
}

/// Repository for versioned quota policies.
pub struct QuotaPolicyRepository;

impl QuotaPolicyRepository {
    /// Apply a quota policy version transactionally while retaining history.
    pub async fn apply_policy(
        pool: &DatabasePool,
        policy: QuotaPolicy,
    ) -> Result<SnapshotApplyOutcome> {
        validate_quota_policy(&policy)?;
        pool.with_conn(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current = load_current_quota_conn(
                &tx,
                &policy.tenant_id,
                policy.workspace_id.as_ref(),
                &policy.dimension,
            )?;
            if let Some(current) = current {
                match snapshot_order(
                    SnapshotIdentity {
                        id: current.policy_id.as_str(),
                        revision: current.source_revision,
                        authority: current.authority,
                        observed_at: current.created_at,
                    },
                    SnapshotIdentity {
                        id: policy.policy_id.as_str(),
                        revision: policy.source_revision,
                        authority: policy.authority,
                        observed_at: policy.created_at,
                    },
                ) {
                    SnapshotOrder::IncomingOlder => {
                        tx.commit()?;
                        return Ok(SnapshotApplyOutcome::IgnoredOlder);
                    }
                    SnapshotOrder::SameIdentity => {
                        if quota_policy_equivalent(&current, &policy) {
                            tx.execute(
                                "UPDATE saas_quota_policies SET stale_marked_at = ?2 \
                                 WHERE policy_id = ?1",
                                rusqlite::params![
                                    policy.policy_id.as_str(),
                                    policy.stale_marked_at.map(|v| v.to_rfc3339()),
                                ],
                            )?;
                            tx.commit()?;
                            return Ok(SnapshotApplyOutcome::Reapplied);
                        }
                        return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                            entity: "quota_policy",
                            revision: policy.source_revision.unwrap_or(0),
                        }));
                    }
                    SnapshotOrder::SameRevisionDifferentIdentity(revision) => {
                        return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                            entity: "quota_policy",
                            revision,
                        }));
                    }
                    SnapshotOrder::IncomingNewer => {}
                }
            }

            if let Some(existing) = load_quota_by_id_conn(&tx, &policy.policy_id)? {
                if quota_policy_equivalent(&existing, &policy) {
                    tx.commit()?;
                    return Ok(SnapshotApplyOutcome::IgnoredOlder);
                }
                return Err(DbError::Domain(MukeiError::SaasRevisionConflict {
                    entity: "quota_policy",
                    revision: policy.source_revision.unwrap_or(0),
                }));
            }

            let workspace = policy.workspace_id.as_ref().map(WorkspaceId::as_str);
            tx.execute(
                "UPDATE saas_quota_policies SET is_current = 0 \
                 WHERE tenant_id = ?1 \
                   AND (workspace_id = ?2 OR (workspace_id IS NULL AND ?2 IS NULL)) \
                   AND dimension = ?3 AND is_current = 1",
                rusqlite::params![policy.tenant_id.as_str(), workspace, policy.dimension.as_str()],
            )?;
            let encoded = encode_window(&policy.window)?;
            tx.execute(
                "INSERT INTO saas_quota_policies (policy_id, tenant_id, workspace_id, dimension, \
                    limit_value, window_kind, rolling_window_seconds, external_start, external_end, \
                    limit_behavior, burst_allowance, warning_threshold, required_entitlement, \
                    authority, source_of_truth, source_revision, stale_marked_at, is_current, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, 1, ?18)",
                rusqlite::params![
                    policy.policy_id.as_str(),
                    policy.tenant_id.as_str(),
                    workspace,
                    policy.dimension.as_str(),
                    to_sql_u64(policy.limit, "quota.limit")?,
                    encoded.kind,
                    encoded.rolling_seconds,
                    encoded.external_start,
                    encoded.external_end,
                    policy.behavior.as_str(),
                    opt_sql_u64(policy.burst_allowance, "quota.burst_allowance")?,
                    opt_sql_u64(policy.warning_threshold, "quota.warning_threshold")?,
                    policy.required_entitlement.as_ref().map(EntitlementKey::as_str),
                    policy.authority.as_str(),
                    policy.source_of_truth.as_str(),
                    opt_sql_u64(policy.source_revision, "quota.source_revision")?,
                    policy.stale_marked_at.map(|v| v.to_rfc3339()),
                    policy.created_at.to_rfc3339(),
                ],
            )?;
            tx.commit()?;
            Ok(SnapshotApplyOutcome::Applied)
        })
        .await
    }

    /// Resolve the current quota policy for a tenant/workspace dimension.
    pub async fn current(
        pool: &DatabasePool,
        tenant_id: TenantId,
        workspace_id: Option<WorkspaceId>,
        dimension: UsageDimension,
    ) -> Result<Option<QuotaPolicy>> {
        pool.with_conn(move |c| {
            load_current_quota_conn(c, &tenant_id, workspace_id.as_ref(), &dimension)
        })
        .await
    }

    /// Mark the current quota policy stale without deleting history.
    pub async fn mark_current_stale(
        pool: &DatabasePool,
        tenant_id: TenantId,
        workspace_id: Option<WorkspaceId>,
        dimension: UsageDimension,
        marked_at: DateTime<Utc>,
    ) -> Result<bool> {
        pool.with_conn(move |c| {
            let workspace = workspace_id.as_ref().map(WorkspaceId::as_str);
            let changed = c.execute(
                "UPDATE saas_quota_policies SET stale_marked_at = ?4 \
                 WHERE tenant_id = ?1 \
                   AND (workspace_id = ?2 OR (workspace_id IS NULL AND ?2 IS NULL)) \
                   AND dimension = ?3 AND is_current = 1",
                rusqlite::params![
                    tenant_id.as_str(),
                    workspace,
                    dimension.as_str(),
                    marked_at.to_rfc3339(),
                ],
            )?;
            Ok::<_, DbError>(changed > 0)
        })
        .await
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RevisionOrder {
    IncomingOlder,
    Same,
    IncomingNewer,
}

fn revision_order(current: Option<RemoteRevision>, incoming: Option<RemoteRevision>) -> RevisionOrder {
    match (current, incoming) {
        (Some(current), Some(incoming)) if incoming < current => RevisionOrder::IncomingOlder,
        (Some(current), Some(incoming)) if incoming == current => RevisionOrder::Same,
        (Some(_), None) => RevisionOrder::IncomingOlder,
        (None, None) => RevisionOrder::Same,
        _ => RevisionOrder::IncomingNewer,
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SnapshotOrder {
    IncomingOlder,
    SameIdentity,
    SameRevisionDifferentIdentity(u64),
    IncomingNewer,
}

struct SnapshotIdentity<'a> {
    id: &'a str,
    revision: Option<RemoteRevision>,
    authority: SnapshotAuthority,
    observed_at: DateTime<Utc>,
}

fn snapshot_order(current: SnapshotIdentity<'_>, incoming: SnapshotIdentity<'_>) -> SnapshotOrder {
    if current.id == incoming.id {
        return SnapshotOrder::SameIdentity;
    }
    if current.authority == SnapshotAuthority::Authoritative
        && incoming.authority == SnapshotAuthority::LocalProvisional
    {
        return SnapshotOrder::IncomingOlder;
    }
    match (current.revision, incoming.revision) {
        (Some(current_revision), Some(incoming_revision))
            if incoming_revision < current_revision =>
        {
            SnapshotOrder::IncomingOlder
        }
        (Some(current_revision), Some(incoming_revision))
            if incoming_revision == current_revision =>
        {
            SnapshotOrder::SameRevisionDifferentIdentity(incoming_revision)
        }
        (Some(_), None) => SnapshotOrder::IncomingOlder,
        (None, Some(_)) => SnapshotOrder::IncomingNewer,
        (None, None) if incoming.observed_at < current.observed_at => SnapshotOrder::IncomingOlder,
        (None, None) if incoming.observed_at == current.observed_at => {
            SnapshotOrder::SameRevisionDifferentIdentity(0)
        }
        _ => SnapshotOrder::IncomingNewer,
    }
}

fn subscription_snapshot_equivalent(a: &SubscriptionState, b: &SubscriptionState) -> bool {
    a.snapshot_id == b.snapshot_id
        && a.tenant_id == b.tenant_id
        && a.plan_key == b.plan_key
        && a.status == b.status
        && a.effective_start == b.effective_start
        && a.effective_end == b.effective_end
        && a.grace_period_end == b.grace_period_end
        && a.source_revision == b.source_revision
        && a.last_synced_at == b.last_synced_at
        && a.authority == b.authority
}

fn entitlement_snapshot_equivalent(a: &EntitlementSnapshot, b: &EntitlementSnapshot) -> bool {
    a.snapshot_id == b.snapshot_id
        && a.tenant_id == b.tenant_id
        && a.grants == b.grants
        && a.source_revision == b.source_revision
        && a.last_synced_at == b.last_synced_at
        && a.authority == b.authority
}

fn quota_policy_equivalent(a: &QuotaPolicy, b: &QuotaPolicy) -> bool {
    a.policy_id == b.policy_id
        && a.tenant_id == b.tenant_id
        && a.workspace_id == b.workspace_id
        && a.dimension == b.dimension
        && a.limit == b.limit
        && a.window == b.window
        && a.behavior == b.behavior
        && a.burst_allowance == b.burst_allowance
        && a.warning_threshold == b.warning_threshold
        && a.required_entitlement == b.required_entitlement
        && a.authority == b.authority
        && a.source_of_truth == b.source_of_truth
        && a.source_revision == b.source_revision
        && a.created_at == b.created_at
}

fn domain_error(error: impl std::fmt::Display) -> MukeiError {
    MukeiError::Invariant(format!("SaaS domain validation failed: {error}"))
}

fn domain_db_error(error: impl std::fmt::Display) -> DbError {
    DbError::Domain(domain_error(error))
}

fn parse_ts(value: String, field: &'static str) -> std::result::Result<DateTime<Utc>, DbError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|_| DbError::Domain(MukeiError::DatabaseInitFailed(format!(
            "invalid SaaS timestamp in {field}"
        ))))
}

fn parse_optional_ts(
    value: Option<String>,
    field: &'static str,
) -> std::result::Result<Option<DateTime<Utc>>, DbError> {
    value.map(|v| parse_ts(v, field)).transpose()
}

fn to_sql_u64(value: u64, field: &'static str) -> std::result::Result<i64, DbError> {
    i64::try_from(value).map_err(|_| {
        DbError::Domain(MukeiError::Invariant(format!(
            "{field} exceeds SQLite INTEGER range"
        )))
    })
}

fn opt_sql_u64(
    value: Option<u64>,
    field: &'static str,
) -> std::result::Result<Option<i64>, DbError> {
    value.map(|v| to_sql_u64(v, field)).transpose()
}

fn from_sql_u64(value: i64, field: &'static str) -> std::result::Result<u64, DbError> {
    u64::try_from(value).map_err(|_| {
        DbError::Domain(MukeiError::DatabaseInitFailed(format!(
            "negative SaaS numeric value in {field}"
        )))
    })
}

fn parse_source(value: &str) -> std::result::Result<SourceOfTruth, DbError> {
    match value {
        "local" => Ok(SourceOfTruth::Local),
        "remote" => Ok(SourceOfTruth::Remote),
        _ => Err(invalid_db_enum("source_of_truth")),
    }
}

fn parse_tenant_status(value: &str) -> std::result::Result<TenantStatus, DbError> {
    match value {
        "active" => Ok(TenantStatus::Active),
        "suspended" => Ok(TenantStatus::Suspended),
        "closed" => Ok(TenantStatus::Closed),
        _ => Err(invalid_db_enum("tenant.status")),
    }
}

fn parse_workspace_status(value: &str) -> std::result::Result<WorkspaceStatus, DbError> {
    match value {
        "active" => Ok(WorkspaceStatus::Active),
        "suspended" => Ok(WorkspaceStatus::Suspended),
        "disabled" => Ok(WorkspaceStatus::Disabled),
        _ => Err(invalid_db_enum("workspace.status")),
    }
}

fn parse_workspace_management(value: &str) -> std::result::Result<WorkspaceManagement, DbError> {
    match value {
        "local_only" => Ok(WorkspaceManagement::LocalOnly),
        "remote_managed" => Ok(WorkspaceManagement::RemoteManaged),
        _ => Err(invalid_db_enum("workspace.management")),
    }
}

fn parse_actor_kind(value: &str) -> std::result::Result<ActorKind, DbError> {
    match value {
        "local" => Ok(ActorKind::Local),
        "human" => Ok(ActorKind::Human),
        "service" => Ok(ActorKind::Service),
        _ => Err(invalid_db_enum("actor.kind")),
    }
}

fn parse_actor_status(value: &str) -> std::result::Result<ActorStatus, DbError> {
    match value {
        "active" => Ok(ActorStatus::Active),
        "suspended" => Ok(ActorStatus::Suspended),
        "disabled" => Ok(ActorStatus::Disabled),
        _ => Err(invalid_db_enum("actor.status")),
    }
}

fn parse_membership_role(value: &str) -> std::result::Result<MembershipRole, DbError> {
    match value {
        "owner" => Ok(MembershipRole::Owner),
        "admin" => Ok(MembershipRole::Admin),
        "member" => Ok(MembershipRole::Member),
        "read_only" => Ok(MembershipRole::ReadOnly),
        _ => Err(invalid_db_enum("membership.role")),
    }
}

fn parse_membership_status(value: &str) -> std::result::Result<MembershipStatus, DbError> {
    match value {
        "active" => Ok(MembershipStatus::Active),
        "suspended" => Ok(MembershipStatus::Suspended),
        "revoked" => Ok(MembershipStatus::Revoked),
        _ => Err(invalid_db_enum("membership.status")),
    }
}

fn parse_subscription_status(value: &str) -> std::result::Result<SubscriptionStatus, DbError> {
    match value {
        "active" => Ok(SubscriptionStatus::Active),
        "trial" => Ok(SubscriptionStatus::Trial),
        "grace_period" => Ok(SubscriptionStatus::GracePeriod),
        "past_due_restricted" => Ok(SubscriptionStatus::PastDueRestricted),
        "cancelled_effective" => Ok(SubscriptionStatus::CancelledEffective),
        "expired" => Ok(SubscriptionStatus::Expired),
        "unknown_stale" => Ok(SubscriptionStatus::UnknownStale),
        _ => Err(invalid_db_enum("subscription.status")),
    }
}

fn parse_authority(value: &str) -> std::result::Result<SnapshotAuthority, DbError> {
    match value {
        "authoritative" => Ok(SnapshotAuthority::Authoritative),
        "local_provisional" => Ok(SnapshotAuthority::LocalProvisional),
        _ => Err(invalid_db_enum("snapshot.authority")),
    }
}

fn parse_usage_kind(value: &str) -> std::result::Result<UsageEventKind, DbError> {
    match value {
        "consumption" => Ok(UsageEventKind::Consumption),
        "adjustment_credit" => Ok(UsageEventKind::AdjustmentCredit),
        _ => Err(invalid_db_enum("usage.event_kind")),
    }
}

fn parse_usage_source(value: &str) -> std::result::Result<UsageSource, DbError> {
    match value {
        "local" => Ok(UsageSource::Local),
        "remote" => Ok(UsageSource::Remote),
        "reconciled" => Ok(UsageSource::Reconciled),
        _ => Err(invalid_db_enum("usage.source")),
    }
}

fn parse_limit_behavior(value: &str) -> std::result::Result<QuotaLimitBehavior, DbError> {
    match value {
        "hard" => Ok(QuotaLimitBehavior::Hard),
        "soft" => Ok(QuotaLimitBehavior::Soft),
        _ => Err(invalid_db_enum("quota.limit_behavior")),
    }
}

fn invalid_db_enum(field: &'static str) -> DbError {
    DbError::Domain(MukeiError::DatabaseInitFailed(format!(
        "invalid SaaS enum value in {field}"
    )))
}

fn id<T, E>(value: String, ctor: impl FnOnce(String) -> std::result::Result<T, E>) -> std::result::Result<T, DbError>
where
    E: std::fmt::Display,
{
    ctor(value).map_err(domain_db_error)
}

fn insert_local_tenant_if_missing(
    tx: &rusqlite::Transaction<'_>,
    tenant: &Tenant,
) -> std::result::Result<(), DbError> {
    tx.execute(
        "INSERT OR IGNORE INTO saas_tenants (tenant_id, display_name, status, source_of_truth, \
            created_at, updated_at, remote_revision, suspension_reason) \
         VALUES (?1, ?2, ?3, 'local', ?4, ?5, NULL, NULL)",
        rusqlite::params![
            tenant.tenant_id.as_str(),
            tenant.display_name,
            tenant.status.as_str(),
            tenant.created_at.to_rfc3339(),
            tenant.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_local_workspace_if_missing(
    tx: &rusqlite::Transaction<'_>,
    workspace: &Workspace,
) -> std::result::Result<(), DbError> {
    tx.execute(
        "INSERT OR IGNORE INTO saas_workspaces (workspace_id, tenant_id, display_name, status, \
            management_mode, created_at, updated_at, remote_revision) \
         VALUES (?1, ?2, ?3, ?4, 'local_only', ?5, ?6, NULL)",
        rusqlite::params![
            workspace.workspace_id.as_str(),
            workspace.tenant_id.as_str(),
            workspace.display_name,
            workspace.status.as_str(),
            workspace.created_at.to_rfc3339(),
            workspace.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_local_actor_if_missing(
    tx: &rusqlite::Transaction<'_>,
    actor: &Actor,
) -> std::result::Result<(), DbError> {
    tx.execute(
        "INSERT OR IGNORE INTO saas_actors (actor_id, display_name, kind, status, source_of_truth, \
            created_at, updated_at, remote_revision) \
         VALUES (?1, ?2, ?3, ?4, 'local', ?5, ?6, NULL)",
        rusqlite::params![
            actor.actor_id.as_str(),
            actor.display_name,
            actor.kind.as_str(),
            actor.status.as_str(),
            actor.created_at.to_rfc3339(),
            actor.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_local_membership_if_missing(
    tx: &rusqlite::Transaction<'_>,
    membership: &WorkspaceMembership,
) -> std::result::Result<(), DbError> {
    tx.execute(
        "INSERT OR IGNORE INTO saas_workspace_memberships (membership_id, tenant_id, workspace_id, \
            actor_id, role, status, created_at, updated_at, remote_revision) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
        rusqlite::params![
            membership.membership_id.as_str(),
            membership.tenant_id.as_str(),
            membership.workspace_id.as_str(),
            membership.actor_id.as_str(),
            membership.role.as_str(),
            membership.status.as_str(),
            membership.created_at.to_rfc3339(),
            membership.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_or_update_local_actor(
    c: &rusqlite::Connection,
    actor: &Actor,
) -> std::result::Result<(), DbError> {
    c.execute(
        "INSERT INTO saas_actors (actor_id, display_name, kind, status, source_of_truth, \
            created_at, updated_at, remote_revision) \
         VALUES (?1, ?2, ?3, ?4, 'local', ?5, ?6, NULL) \
         ON CONFLICT(actor_id) DO UPDATE SET \
            display_name = excluded.display_name, kind = excluded.kind, status = excluded.status, \
            updated_at = excluded.updated_at \
         WHERE saas_actors.source_of_truth = 'local'",
        rusqlite::params![
            actor.actor_id.as_str(),
            actor.display_name,
            actor.kind.as_str(),
            actor.status.as_str(),
            actor.created_at.to_rfc3339(),
            actor.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

struct TenantRow {
    tenant_id: String,
    display_name: String,
    status: String,
    source_of_truth: String,
    created_at: String,
    updated_at: String,
    remote_revision: Option<i64>,
    suspension_reason: Option<String>,
}

struct WorkspaceRow {
    workspace_id: String,
    tenant_id: String,
    display_name: String,
    status: String,
    management_mode: String,
    created_at: String,
    updated_at: String,
    remote_revision: Option<i64>,
}

struct ActorRow {
    actor_id: String,
    display_name: Option<String>,
    kind: String,
    status: String,
    source_of_truth: String,
    created_at: String,
    updated_at: String,
    remote_revision: Option<i64>,
}

struct MembershipRow {
    membership_id: String,
    tenant_id: String,
    workspace_id: String,
    actor_id: String,
    role: String,
    status: String,
    created_at: String,
    updated_at: String,
    remote_revision: Option<i64>,
}

fn load_tenant_conn(
    c: &rusqlite::Connection,
    tenant_id: &TenantId,
) -> std::result::Result<Option<Tenant>, DbError> {
    let raw = c
        .query_row(
            "SELECT tenant_id, display_name, status, source_of_truth, created_at, updated_at, \
                    remote_revision, suspension_reason \
             FROM saas_tenants WHERE tenant_id = ?1",
            [tenant_id.as_str()],
            |row| {
                Ok(TenantRow {
                    tenant_id: row.get(0)?,
                    display_name: row.get(1)?,
                    status: row.get(2)?,
                    source_of_truth: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    remote_revision: row.get(6)?,
                    suspension_reason: row.get(7)?,
                })
            },
        )
        .optional()?;
    raw.map(|row| {
        Ok(Tenant {
            tenant_id: id(row.tenant_id, TenantId::new)?,
            display_name: row.display_name,
            status: parse_tenant_status(&row.status)?,
            source_of_truth: parse_source(&row.source_of_truth)?,
            created_at: parse_ts(row.created_at, "tenant.created_at")?,
            updated_at: parse_ts(row.updated_at, "tenant.updated_at")?,
            remote_revision: row
                .remote_revision
                .map(|value| from_sql_u64(value, "tenant.remote_revision"))
                .transpose()?,
            suspension_reason: row.suspension_reason,
        })
    })
    .transpose()
}

fn load_workspace_conn(
    c: &rusqlite::Connection,
    workspace_id: &WorkspaceId,
) -> std::result::Result<Option<Workspace>, DbError> {
    let raw = c
        .query_row(
            "SELECT workspace_id, tenant_id, display_name, status, management_mode, created_at, \
                    updated_at, remote_revision \
             FROM saas_workspaces WHERE workspace_id = ?1",
            [workspace_id.as_str()],
            |row| {
                Ok(WorkspaceRow {
                    workspace_id: row.get(0)?,
                    tenant_id: row.get(1)?,
                    display_name: row.get(2)?,
                    status: row.get(3)?,
                    management_mode: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    remote_revision: row.get(7)?,
                })
            },
        )
        .optional()?;
    raw.map(|row| {
        Ok(Workspace {
            workspace_id: id(row.workspace_id, WorkspaceId::new)?,
            tenant_id: id(row.tenant_id, TenantId::new)?,
            display_name: row.display_name,
            status: parse_workspace_status(&row.status)?,
            management: parse_workspace_management(&row.management_mode)?,
            created_at: parse_ts(row.created_at, "workspace.created_at")?,
            updated_at: parse_ts(row.updated_at, "workspace.updated_at")?,
            remote_revision: row
                .remote_revision
                .map(|value| from_sql_u64(value, "workspace.remote_revision"))
                .transpose()?,
        })
    })
    .transpose()
}

fn load_actor_conn(
    c: &rusqlite::Connection,
    actor_id: &ActorId,
) -> std::result::Result<Option<Actor>, DbError> {
    let raw = c
        .query_row(
            "SELECT actor_id, display_name, kind, status, source_of_truth, created_at, updated_at, \
                    remote_revision FROM saas_actors WHERE actor_id = ?1",
            [actor_id.as_str()],
            |row| {
                Ok(ActorRow {
                    actor_id: row.get(0)?,
                    display_name: row.get(1)?,
                    kind: row.get(2)?,
                    status: row.get(3)?,
                    source_of_truth: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    remote_revision: row.get(7)?,
                })
            },
        )
        .optional()?;
    raw.map(|row| {
        Ok(Actor {
            actor_id: id(row.actor_id, ActorId::new)?,
            display_name: row.display_name,
            kind: parse_actor_kind(&row.kind)?,
            status: parse_actor_status(&row.status)?,
            source_of_truth: parse_source(&row.source_of_truth)?,
            created_at: parse_ts(row.created_at, "actor.created_at")?,
            updated_at: parse_ts(row.updated_at, "actor.updated_at")?,
            remote_revision: row
                .remote_revision
                .map(|value| from_sql_u64(value, "actor.remote_revision"))
                .transpose()?,
        })
    })
    .transpose()
}

fn load_membership_conn(
    c: &rusqlite::Connection,
    membership_id: &MembershipId,
) -> std::result::Result<Option<WorkspaceMembership>, DbError> {
    let raw = c
        .query_row(
            "SELECT membership_id, tenant_id, workspace_id, actor_id, role, status, created_at, \
                    updated_at, remote_revision \
             FROM saas_workspace_memberships WHERE membership_id = ?1",
            [membership_id.as_str()],
            |row| {
                Ok(MembershipRow {
                    membership_id: row.get(0)?,
                    tenant_id: row.get(1)?,
                    workspace_id: row.get(2)?,
                    actor_id: row.get(3)?,
                    role: row.get(4)?,
                    status: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                    remote_revision: row.get(8)?,
                })
            },
        )
        .optional()?;
    raw.map(|row| {
        Ok(WorkspaceMembership {
            membership_id: id(row.membership_id, MembershipId::new)?,
            tenant_id: id(row.tenant_id, TenantId::new)?,
            workspace_id: id(row.workspace_id, WorkspaceId::new)?,
            actor_id: id(row.actor_id, ActorId::new)?,
            role: parse_membership_role(&row.role)?,
            status: parse_membership_status(&row.status)?,
            created_at: parse_ts(row.created_at, "membership.created_at")?,
            updated_at: parse_ts(row.updated_at, "membership.updated_at")?,
            remote_revision: row
                .remote_revision
                .map(|value| from_sql_u64(value, "membership.remote_revision"))
                .transpose()?,
        })
    })
    .transpose()
}

fn validate_tenant(tenant: &Tenant) -> Result<()> {
    validate_display_name(&tenant.display_name, "tenant.display_name")?;
    if tenant.updated_at < tenant.created_at {
        return Err(MukeiError::Invariant(
            "tenant updated_at must not precede created_at".into(),
        ));
    }
    if let Some(reason) = tenant.suspension_reason.as_deref() {
        validate_machine_token(reason, "tenant.suspension_reason", 128)?;
    }
    Ok(())
}

fn validate_workspace(workspace: &Workspace) -> Result<()> {
    validate_display_name(&workspace.display_name, "workspace.display_name")?;
    if workspace.updated_at < workspace.created_at {
        return Err(MukeiError::Invariant(
            "workspace updated_at must not precede created_at".into(),
        ));
    }
    Ok(())
}

fn validate_actor(actor: &Actor) -> Result<()> {
    if actor.display_name.as_ref().is_some_and(|value| value.len() > 256) {
        return Err(MukeiError::Invariant(
            "actor display_name exceeds 256 bytes".into(),
        ));
    }
    if actor.updated_at < actor.created_at {
        return Err(MukeiError::Invariant(
            "actor updated_at must not precede created_at".into(),
        ));
    }
    Ok(())
}

fn validate_membership(membership: &WorkspaceMembership) -> Result<()> {
    if membership.updated_at < membership.created_at {
        return Err(MukeiError::Invariant(
            "membership updated_at must not precede created_at".into(),
        ));
    }
    Ok(())
}

fn validate_display_name(value: &str, field: &'static str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 {
        return Err(MukeiError::Invariant(format!(
            "{field} must be non-empty and at most 256 bytes"
        )));
    }
    Ok(())
}

fn validate_machine_token(value: &str, field: &'static str, max_bytes: usize) -> Result<()> {
    if value.is_empty() || value.len() > max_bytes {
        return Err(MukeiError::Invariant(format!(
            "{field} must be 1..={max_bytes} bytes"
        )));
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':' | b'/'))
    {
        return Err(MukeiError::Invariant(format!(
            "{field} must be a machine token, not localized/free-form text"
        )));
    }
    Ok(())
}

fn validate_subscription(snapshot: &SubscriptionState) -> Result<()> {
    validate_machine_token(&snapshot.plan_key, "subscription.plan_key", 128)?;
    if snapshot.effective_end.is_some_and(|end| end <= snapshot.effective_start) {
        return Err(MukeiError::Invariant(
            "subscription effective_end must follow effective_start".into(),
        ));
    }
    Ok(())
}

type RawSubscription = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    String,
    String,
    Option<String>,
);

fn load_current_subscription_conn(
    c: &rusqlite::Connection,
    tenant_id: &TenantId,
) -> std::result::Result<Option<SubscriptionState>, DbError> {
    load_subscription_where(c, "tenant_id = ?1 AND is_current = 1", tenant_id.as_str())
}

fn load_subscription_by_id_conn(
    c: &rusqlite::Connection,
    snapshot_id: &SubscriptionSnapshotId,
) -> std::result::Result<Option<SubscriptionState>, DbError> {
    load_subscription_where(c, "snapshot_id = ?1", snapshot_id.as_str())
}

fn load_subscription_where(
    c: &rusqlite::Connection,
    predicate: &str,
    value: &str,
) -> std::result::Result<Option<SubscriptionState>, DbError> {
    let sql = format!(
        "SELECT snapshot_id, tenant_id, plan_key, status, effective_start, effective_end, \
                grace_period_end, source_revision, last_synced_at, authority, stale_marked_at \
         FROM saas_subscription_snapshots WHERE {predicate}"
    );
    let raw: Option<RawSubscription> = c
        .query_row(&sql, [value], |row| {
            Ok((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?,
                row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?,
            ))
        })
        .optional()?;
    raw.map(parse_subscription_raw).transpose()
}

fn parse_subscription_raw(r: RawSubscription) -> std::result::Result<SubscriptionState, DbError> {
    Ok(SubscriptionState {
        snapshot_id: id(r.0, SubscriptionSnapshotId::new)?,
        tenant_id: id(r.1, TenantId::new)?,
        plan_key: r.2,
        status: parse_subscription_status(&r.3)?,
        effective_start: parse_ts(r.4, "subscription.effective_start")?,
        effective_end: parse_optional_ts(r.5, "subscription.effective_end")?,
        grace_period_end: parse_optional_ts(r.6, "subscription.grace_period_end")?,
        source_revision: r.7.map(|v| from_sql_u64(v, "subscription.source_revision")).transpose()?,
        last_synced_at: parse_ts(r.8, "subscription.last_synced_at")?,
        authority: parse_authority(&r.9)?,
        stale_marked_at: parse_optional_ts(r.10, "subscription.stale_marked_at")?,
    })
}

fn validate_entitlement_snapshot(snapshot: &EntitlementSnapshot) -> Result<()> {
    for (key, grant) in &snapshot.grants {
        if key != &grant.key {
            return Err(MukeiError::Invariant(
                "entitlement map key must match grant.key".into(),
            ));
        }
        if grant.string_value.as_ref().is_some_and(|v| v.len() > 512) {
            return Err(MukeiError::Invariant(
                "entitlement string value exceeds 512 bytes".into(),
            ));
        }
        if grant
            .effective_start
            .zip(grant.effective_end)
            .is_some_and(|(start, end)| end <= start)
        {
            return Err(MukeiError::Invariant(
                "entitlement effective_end must follow effective_start".into(),
            ));
        }
        if let Some(limit) = grant.numeric_limit {
            let _ = i64::try_from(limit).map_err(|_| {
                MukeiError::Invariant("entitlement numeric limit exceeds SQLite range".into())
            })?;
        }
    }
    Ok(())
}

type RawEntitlementHeader = (String, String, Option<i64>, String, String, Option<String>);

fn load_current_entitlement_conn(
    c: &rusqlite::Connection,
    tenant_id: &TenantId,
) -> std::result::Result<Option<EntitlementSnapshot>, DbError> {
    load_entitlement_where(c, "tenant_id = ?1 AND is_current = 1", tenant_id.as_str())
}

fn load_entitlement_by_id_conn(
    c: &rusqlite::Connection,
    snapshot_id: &EntitlementSnapshotId,
) -> std::result::Result<Option<EntitlementSnapshot>, DbError> {
    load_entitlement_where(c, "snapshot_id = ?1", snapshot_id.as_str())
}

fn load_entitlement_where(
    c: &rusqlite::Connection,
    predicate: &str,
    value: &str,
) -> std::result::Result<Option<EntitlementSnapshot>, DbError> {
    let sql = format!(
        "SELECT snapshot_id, tenant_id, source_revision, last_synced_at, authority, stale_marked_at \
         FROM saas_entitlement_snapshots WHERE {predicate}"
    );
    let raw: Option<RawEntitlementHeader> = c
        .query_row(&sql, [value], |row| {
            Ok((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?,
            ))
        })
        .optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let snapshot_id = id(raw.0, EntitlementSnapshotId::new)?;
    let mut stmt = c.prepare(
        "SELECT entitlement_key, enabled, numeric_limit, string_value, effective_start, \
                effective_end, source_revision \
         FROM saas_entitlement_grants WHERE snapshot_id = ?1 ORDER BY entitlement_key",
    )?;
    let rows = stmt.query_map([snapshot_id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<i64>>(6)?,
        ))
    })?;
    let mut grants = BTreeMap::new();
    for row in rows {
        let row = row?;
        let key = id(row.0, EntitlementKey::new)?;
        let grant = EntitlementGrant {
            key: key.clone(),
            enabled: row.1 != 0,
            numeric_limit: row.2.map(|v| from_sql_u64(v, "entitlement.numeric_limit")).transpose()?,
            string_value: row.3,
            effective_start: parse_optional_ts(row.4, "entitlement.effective_start")?,
            effective_end: parse_optional_ts(row.5, "entitlement.effective_end")?,
            source_revision: row.6.map(|v| from_sql_u64(v, "entitlement.grant_revision")).transpose()?,
        };
        grants.insert(key, grant);
    }
    Ok(Some(EntitlementSnapshot {
        snapshot_id,
        tenant_id: id(raw.1, TenantId::new)?,
        grants,
        source_revision: raw.2.map(|v| from_sql_u64(v, "entitlement.source_revision")).transpose()?,
        last_synced_at: parse_ts(raw.3, "entitlement.last_synced_at")?,
        authority: parse_authority(&raw.4)?,
        stale_marked_at: parse_optional_ts(raw.5, "entitlement.stale_marked_at")?,
    }))
}

type RawUsage = (
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    String,
);

fn load_usage_by_id_conn(
    c: &rusqlite::Connection,
    usage_event_id: &UsageEventId,
) -> std::result::Result<Option<UsageEvent>, DbError> {
    let raw: Option<RawUsage> = c
        .query_row(
            usage_select("usage_event_id = ?1").as_str(),
            [usage_event_id.as_str()],
            raw_usage_row,
        )
        .optional()?;
    raw.map(parse_usage_raw).transpose()
}

fn load_usage_by_idempotency_conn(
    c: &rusqlite::Connection,
    tenant_id: &TenantId,
    dimension: &UsageDimension,
    key: &IdempotencyKey,
) -> std::result::Result<Option<UsageEvent>, DbError> {
    let raw: Option<RawUsage> = c
        .query_row(
            usage_select("tenant_id = ?1 AND dimension = ?2 AND idempotency_key = ?3").as_str(),
            rusqlite::params![tenant_id.as_str(), dimension.as_str(), key.as_str()],
            raw_usage_row,
        )
        .optional()?;
    raw.map(parse_usage_raw).transpose()
}

fn usage_select(predicate: &str) -> String {
    format!(
        "SELECT usage_event_id, tenant_id, workspace_id, actor_id, dimension, quantity, \
                event_kind, occurred_at, recorded_at, operation_id, correlation_id, \
                idempotency_key, source, adjusts_usage_event_id, metadata_json \
         FROM saas_usage_events WHERE {predicate}"
    )
}

fn raw_usage_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawUsage> {
    Ok((
        row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?,
        row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?,
        row.get(12)?, row.get(13)?, row.get(14)?,
    ))
}

fn parse_usage_raw(r: RawUsage) -> std::result::Result<UsageEvent, DbError> {
    let values: BTreeMap<String, crate::saas::UsageMetadataValue> =
        serde_json::from_str(&r.14).map_err(|_| {
            DbError::Domain(MukeiError::DatabaseInitFailed(
                "invalid bounded usage metadata JSON".into(),
            ))
        })?;
    Ok(UsageEvent {
        usage_event_id: id(r.0, UsageEventId::new)?,
        tenant_id: id(r.1, TenantId::new)?,
        workspace_id: r.2.map(|v| id(v, WorkspaceId::new)).transpose()?,
        actor_id: r.3.map(|v| id(v, ActorId::new)).transpose()?,
        dimension: id(r.4, UsageDimension::new)?,
        quantity: from_sql_u64(r.5, "usage.quantity")?,
        kind: parse_usage_kind(&r.6)?,
        occurred_at: parse_ts(r.7, "usage.occurred_at")?,
        recorded_at: parse_ts(r.8, "usage.recorded_at")?,
        operation_id: r.9.map(|v| id(v, OperationId::new)).transpose()?,
        correlation_id: r.10.map(|v| id(v, CorrelationId::new)).transpose()?,
        idempotency_key: id(r.11, IdempotencyKey::new)?,
        source: parse_usage_source(&r.12)?,
        adjusts_usage_event_id: r.13.map(|v| id(v, UsageEventId::new)).transpose()?,
        metadata: UsageMetadata::new(values).map_err(domain_db_error)?,
    })
}

fn validate_adjustment_credit_conn(
    c: &rusqlite::Connection,
    event: &UsageEvent,
) -> std::result::Result<(), DbError> {
    if event.kind != UsageEventKind::AdjustmentCredit {
        return Ok(());
    }
    let original_id = event.adjusts_usage_event_id.as_ref().ok_or_else(|| {
        DbError::Domain(MukeiError::Invariant(
            "adjustment credit requires an original usage event".into(),
        ))
    })?;
    let original = load_usage_by_id_conn(c, original_id)?.ok_or_else(|| {
        DbError::Domain(MukeiError::Invariant(
            "adjustment credit references a missing usage event".into(),
        ))
    })?;
    if original.kind != UsageEventKind::Consumption
        || original.tenant_id != event.tenant_id
        || original.workspace_id != event.workspace_id
        || original.dimension != event.dimension
    {
        return Err(DbError::Domain(MukeiError::Invariant(
            "adjustment credit must reference consumption in the same tenant, workspace, and dimension".into(),
        )));
    }
    let credited_sql: i64 = c.query_row(
        "SELECT COALESCE(SUM(quantity), 0) FROM saas_usage_events \
         WHERE event_kind = 'adjustment_credit' AND adjusts_usage_event_id = ?1",
        [original_id.as_str()],
        |row| row.get(0),
    )?;
    let credited = from_sql_u64(credited_sql, "usage.adjustment_credits")?;
    let projected = credited.checked_add(event.quantity).ok_or_else(|| {
        DbError::Domain(MukeiError::Invariant(
            "usage adjustment arithmetic overflow".into(),
        ))
    })?;
    if projected > original.quantity {
        return Err(DbError::Domain(MukeiError::Invariant(
            "usage adjustment would underflow the original consumption".into(),
        )));
    }
    Ok(())
}

fn usage_mutation_equivalent(a: &UsageEvent, b: &UsageEvent) -> bool {
    a.tenant_id == b.tenant_id
        && a.workspace_id == b.workspace_id
        && a.actor_id == b.actor_id
        && a.dimension == b.dimension
        && a.quantity == b.quantity
        && a.kind == b.kind
        && a.occurred_at == b.occurred_at
        && a.operation_id == b.operation_id
        && a.source == b.source
        && a.adjusts_usage_event_id == b.adjusts_usage_event_id
        && a.metadata == b.metadata
}

struct EncodedWindow {
    kind: &'static str,
    rolling_seconds: Option<i64>,
    external_start: Option<String>,
    external_end: Option<String>,
}

fn encode_window(window: &UsageWindow) -> std::result::Result<EncodedWindow, DbError> {
    match window {
        UsageWindow::Lifetime => Ok(EncodedWindow {
            kind: "lifetime",
            rolling_seconds: None,
            external_start: None,
            external_end: None,
        }),
        UsageWindow::CalendarDay => Ok(EncodedWindow {
            kind: "calendar_day",
            rolling_seconds: None,
            external_start: None,
            external_end: None,
        }),
        UsageWindow::CalendarMonth => Ok(EncodedWindow {
            kind: "calendar_month",
            rolling_seconds: None,
            external_start: None,
            external_end: None,
        }),
        UsageWindow::Rolling { seconds } => {
            if *seconds == 0 {
                return Err(DbError::Domain(MukeiError::Invariant(
                    "rolling quota window must be positive".into(),
                )));
            }
            Ok(EncodedWindow {
                kind: "rolling",
                rolling_seconds: Some(to_sql_u64(*seconds, "quota.rolling_seconds")?),
                external_start: None,
                external_end: None,
            })
        }
        UsageWindow::ExternalPeriod { start, end } => {
            if end <= start {
                return Err(DbError::Domain(MukeiError::Invariant(
                    "external quota period end must follow start".into(),
                )));
            }
            Ok(EncodedWindow {
                kind: "external_period",
                rolling_seconds: None,
                external_start: Some(start.to_rfc3339()),
                external_end: Some(end.to_rfc3339()),
            })
        }
    }
}

fn parse_window(
    kind: &str,
    rolling_seconds: Option<i64>,
    external_start: Option<String>,
    external_end: Option<String>,
) -> std::result::Result<UsageWindow, DbError> {
    match kind {
        "lifetime" => Ok(UsageWindow::Lifetime),
        "calendar_day" => Ok(UsageWindow::CalendarDay),
        "calendar_month" => Ok(UsageWindow::CalendarMonth),
        "rolling" => Ok(UsageWindow::Rolling {
            seconds: from_sql_u64(
                rolling_seconds.ok_or_else(|| invalid_db_enum("quota.rolling_seconds"))?,
                "quota.rolling_seconds",
            )?,
        }),
        "external_period" => Ok(UsageWindow::ExternalPeriod {
            start: parse_ts(
                external_start.ok_or_else(|| invalid_db_enum("quota.external_start"))?,
                "quota.external_start",
            )?,
            end: parse_ts(
                external_end.ok_or_else(|| invalid_db_enum("quota.external_end"))?,
                "quota.external_end",
            )?,
        }),
        _ => Err(invalid_db_enum("quota.window_kind")),
    }
}

fn validate_quota_policy(policy: &QuotaPolicy) -> Result<()> {
    let _ = i64::try_from(policy.limit)
        .map_err(|_| MukeiError::Invariant("quota limit exceeds SQLite range".into()))?;
    if let Some(value) = policy.burst_allowance {
        let _ = i64::try_from(value)
            .map_err(|_| MukeiError::Invariant("quota burst exceeds SQLite range".into()))?;
    }
    if let Some(value) = policy.warning_threshold {
        let _ = i64::try_from(value).map_err(|_| {
            MukeiError::Invariant("quota warning threshold exceeds SQLite range".into())
        })?;
    }
    policy
        .window
        .bounds_at(Utc::now())
        .map_err(domain_error)?;
    Ok(())
}

type RawQuota = (
    String,
    String,
    Option<String>,
    String,
    i64,
    String,
    Option<i64>,
    Option<String>,
    Option<String>,
    String,
    Option<i64>,
    Option<i64>,
    Option<String>,
    String,
    String,
    Option<i64>,
    Option<String>,
    String,
);

fn load_current_quota_conn(
    c: &rusqlite::Connection,
    tenant_id: &TenantId,
    workspace_id: Option<&WorkspaceId>,
    dimension: &UsageDimension,
) -> std::result::Result<Option<QuotaPolicy>, DbError> {
    let workspace = workspace_id.map(WorkspaceId::as_str);
    let raw: Option<RawQuota> = c
        .query_row(
            "SELECT policy_id, tenant_id, workspace_id, dimension, limit_value, window_kind, \
                    rolling_window_seconds, external_start, external_end, limit_behavior, \
                    burst_allowance, warning_threshold, required_entitlement, authority, \
                    source_of_truth, source_revision, stale_marked_at, created_at \
             FROM saas_quota_policies \
             WHERE tenant_id = ?1 \
               AND (workspace_id = ?2 OR (workspace_id IS NULL AND ?2 IS NULL)) \
               AND dimension = ?3 AND is_current = 1",
            rusqlite::params![tenant_id.as_str(), workspace, dimension.as_str()],
            raw_quota_row,
        )
        .optional()?;
    raw.map(parse_quota_raw).transpose()
}

fn load_quota_by_id_conn(
    c: &rusqlite::Connection,
    policy_id: &QuotaPolicyId,
) -> std::result::Result<Option<QuotaPolicy>, DbError> {
    let raw: Option<RawQuota> = c
        .query_row(
            "SELECT policy_id, tenant_id, workspace_id, dimension, limit_value, window_kind, \
                    rolling_window_seconds, external_start, external_end, limit_behavior, \
                    burst_allowance, warning_threshold, required_entitlement, authority, \
                    source_of_truth, source_revision, stale_marked_at, created_at \
             FROM saas_quota_policies WHERE policy_id = ?1",
            [policy_id.as_str()],
            raw_quota_row,
        )
        .optional()?;
    raw.map(parse_quota_raw).transpose()
}

fn raw_quota_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawQuota> {
    Ok((
        row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?,
        row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?,
        row.get(12)?, row.get(13)?, row.get(14)?, row.get(15)?, row.get(16)?, row.get(17)?,
    ))
}

fn parse_quota_raw(r: RawQuota) -> std::result::Result<QuotaPolicy, DbError> {
    Ok(QuotaPolicy {
        policy_id: id(r.0, QuotaPolicyId::new)?,
        tenant_id: id(r.1, TenantId::new)?,
        workspace_id: r.2.map(|v| id(v, WorkspaceId::new)).transpose()?,
        dimension: id(r.3, UsageDimension::new)?,
        limit: from_sql_u64(r.4, "quota.limit")?,
        window: parse_window(&r.5, r.6, r.7, r.8)?,
        behavior: parse_limit_behavior(&r.9)?,
        burst_allowance: r.10.map(|v| from_sql_u64(v, "quota.burst_allowance")).transpose()?,
        warning_threshold: r.11.map(|v| from_sql_u64(v, "quota.warning_threshold")).transpose()?,
        required_entitlement: r.12.map(|v| id(v, EntitlementKey::new)).transpose()?,
        authority: parse_authority(&r.13)?,
        source_of_truth: parse_source(&r.14)?,
        source_revision: r.15.map(|v| from_sql_u64(v, "quota.source_revision")).transpose()?,
        stale_marked_at: parse_optional_ts(r.16, "quota.stale_marked_at")?,
        created_at: parse_ts(r.17, "quota.created_at")?,
    })
}
