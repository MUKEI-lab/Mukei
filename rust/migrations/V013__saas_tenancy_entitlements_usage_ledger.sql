-- V013__saas_tenancy_entitlements_usage_ledger.sql
-- Local-first SaaS domain foundation: tenancy, workspace membership,
-- provider-neutral subscription/entitlement snapshots, immutable usage ledger,
-- and versioned quota policies. Existing conversation/document tables remain
-- intentionally untouched.

CREATE TABLE IF NOT EXISTS saas_tenants (
    tenant_id           TEXT PRIMARY KEY CHECK (length(trim(tenant_id)) > 0),
    display_name        TEXT NOT NULL CHECK (length(trim(display_name)) > 0 AND length(display_name) <= 256),
    status              TEXT NOT NULL CHECK (status IN ('active', 'suspended', 'closed')),
    source_of_truth     TEXT NOT NULL CHECK (source_of_truth IN ('local', 'remote')),
    created_at          TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at          TEXT NOT NULL CHECK (length(updated_at) > 0),
    remote_revision     INTEGER CHECK (remote_revision IS NULL OR remote_revision >= 0),
    suspension_reason   TEXT CHECK (suspension_reason IS NULL OR length(suspension_reason) <= 128)
);

CREATE TABLE IF NOT EXISTS saas_workspaces (
    workspace_id        TEXT PRIMARY KEY CHECK (length(trim(workspace_id)) > 0),
    tenant_id           TEXT NOT NULL,
    display_name        TEXT NOT NULL CHECK (length(trim(display_name)) > 0 AND length(display_name) <= 256),
    status              TEXT NOT NULL CHECK (status IN ('active', 'suspended', 'disabled')),
    management_mode     TEXT NOT NULL CHECK (management_mode IN ('local_only', 'remote_managed')),
    created_at          TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at          TEXT NOT NULL CHECK (length(updated_at) > 0),
    remote_revision     INTEGER CHECK (remote_revision IS NULL OR remote_revision >= 0),
    UNIQUE (workspace_id, tenant_id),
    FOREIGN KEY (tenant_id) REFERENCES saas_tenants(tenant_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_saas_workspaces_tenant
    ON saas_workspaces(tenant_id, status);

CREATE TABLE IF NOT EXISTS saas_actors (
    actor_id            TEXT PRIMARY KEY CHECK (length(trim(actor_id)) > 0),
    display_name        TEXT CHECK (display_name IS NULL OR length(display_name) <= 256),
    kind                TEXT NOT NULL CHECK (kind IN ('local', 'human', 'service')),
    status              TEXT NOT NULL CHECK (status IN ('active', 'suspended', 'disabled')),
    source_of_truth     TEXT NOT NULL CHECK (source_of_truth IN ('local', 'remote')),
    created_at          TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at          TEXT NOT NULL CHECK (length(updated_at) > 0),
    remote_revision     INTEGER CHECK (remote_revision IS NULL OR remote_revision >= 0)
);

CREATE TABLE IF NOT EXISTS saas_workspace_memberships (
    membership_id       TEXT PRIMARY KEY CHECK (length(trim(membership_id)) > 0),
    tenant_id           TEXT NOT NULL,
    workspace_id        TEXT NOT NULL,
    actor_id            TEXT NOT NULL,
    role                TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'read_only')),
    status              TEXT NOT NULL CHECK (status IN ('active', 'suspended', 'revoked')),
    created_at          TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at          TEXT NOT NULL CHECK (length(updated_at) > 0),
    remote_revision     INTEGER CHECK (remote_revision IS NULL OR remote_revision >= 0),
    UNIQUE (workspace_id, actor_id),
    FOREIGN KEY (tenant_id) REFERENCES saas_tenants(tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (workspace_id, tenant_id)
        REFERENCES saas_workspaces(workspace_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_id) REFERENCES saas_actors(actor_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_saas_memberships_actor
    ON saas_workspace_memberships(actor_id, status);
CREATE INDEX IF NOT EXISTS idx_saas_memberships_workspace
    ON saas_workspace_memberships(workspace_id, status);

CREATE TABLE IF NOT EXISTS saas_subscription_snapshots (
    snapshot_id         TEXT PRIMARY KEY CHECK (length(trim(snapshot_id)) > 0),
    tenant_id           TEXT NOT NULL,
    plan_key            TEXT NOT NULL CHECK (length(trim(plan_key)) > 0 AND length(plan_key) <= 128),
    status              TEXT NOT NULL CHECK (
        status IN ('active', 'trial', 'grace_period', 'past_due_restricted',
                   'cancelled_effective', 'expired', 'unknown_stale')
    ),
    effective_start     TEXT NOT NULL CHECK (length(effective_start) > 0),
    effective_end       TEXT,
    grace_period_end    TEXT,
    source_revision     INTEGER CHECK (source_revision IS NULL OR source_revision >= 0),
    last_synced_at      TEXT NOT NULL CHECK (length(last_synced_at) > 0),
    authority           TEXT NOT NULL CHECK (authority IN ('authoritative', 'local_provisional')),
    stale_marked_at     TEXT,
    is_current          INTEGER NOT NULL DEFAULT 0 CHECK (is_current IN (0, 1)),
    created_at          TEXT NOT NULL CHECK (length(created_at) > 0),
    FOREIGN KEY (tenant_id) REFERENCES saas_tenants(tenant_id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_saas_subscription_current_per_tenant
    ON saas_subscription_snapshots(tenant_id)
    WHERE is_current = 1;
CREATE UNIQUE INDEX IF NOT EXISTS uq_saas_subscription_remote_revision
    ON saas_subscription_snapshots(tenant_id, source_revision)
    WHERE source_revision IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_saas_subscription_tenant_history
    ON saas_subscription_snapshots(tenant_id, created_at DESC);

CREATE TABLE IF NOT EXISTS saas_entitlement_snapshots (
    snapshot_id         TEXT PRIMARY KEY CHECK (length(trim(snapshot_id)) > 0),
    tenant_id           TEXT NOT NULL,
    source_revision     INTEGER CHECK (source_revision IS NULL OR source_revision >= 0),
    last_synced_at      TEXT NOT NULL CHECK (length(last_synced_at) > 0),
    authority           TEXT NOT NULL CHECK (authority IN ('authoritative', 'local_provisional')),
    stale_marked_at     TEXT,
    is_current          INTEGER NOT NULL DEFAULT 0 CHECK (is_current IN (0, 1)),
    created_at          TEXT NOT NULL CHECK (length(created_at) > 0),
    FOREIGN KEY (tenant_id) REFERENCES saas_tenants(tenant_id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_saas_entitlement_current_per_tenant
    ON saas_entitlement_snapshots(tenant_id)
    WHERE is_current = 1;
CREATE UNIQUE INDEX IF NOT EXISTS uq_saas_entitlement_remote_revision
    ON saas_entitlement_snapshots(tenant_id, source_revision)
    WHERE source_revision IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_saas_entitlement_tenant_history
    ON saas_entitlement_snapshots(tenant_id, created_at DESC);

CREATE TABLE IF NOT EXISTS saas_entitlement_grants (
    snapshot_id         TEXT NOT NULL,
    entitlement_key     TEXT NOT NULL CHECK (length(trim(entitlement_key)) > 0 AND length(entitlement_key) <= 128),
    enabled             INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    numeric_limit       INTEGER CHECK (numeric_limit IS NULL OR numeric_limit >= 0),
    string_value        TEXT CHECK (string_value IS NULL OR length(string_value) <= 512),
    effective_start     TEXT,
    effective_end       TEXT,
    source_revision     INTEGER CHECK (source_revision IS NULL OR source_revision >= 0),
    PRIMARY KEY (snapshot_id, entitlement_key),
    FOREIGN KEY (snapshot_id) REFERENCES saas_entitlement_snapshots(snapshot_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS saas_usage_events (
    usage_event_id          TEXT PRIMARY KEY CHECK (length(trim(usage_event_id)) > 0),
    tenant_id               TEXT NOT NULL,
    workspace_id            TEXT,
    actor_id                TEXT,
    dimension               TEXT NOT NULL CHECK (length(trim(dimension)) > 0 AND length(dimension) <= 128),
    quantity                INTEGER NOT NULL CHECK (quantity >= 0),
    event_kind              TEXT NOT NULL CHECK (event_kind IN ('consumption', 'adjustment_credit')),
    occurred_at             TEXT NOT NULL CHECK (length(occurred_at) > 0),
    recorded_at             TEXT NOT NULL CHECK (length(recorded_at) > 0),
    operation_id            TEXT CHECK (operation_id IS NULL OR length(trim(operation_id)) > 0),
    correlation_id          TEXT CHECK (correlation_id IS NULL OR length(trim(correlation_id)) > 0),
    idempotency_key         TEXT NOT NULL CHECK (length(trim(idempotency_key)) > 0),
    source                  TEXT NOT NULL CHECK (source IN ('local', 'remote', 'reconciled')),
    adjusts_usage_event_id  TEXT,
    metadata_json           TEXT NOT NULL DEFAULT '{}' CHECK (length(metadata_json) <= 4096),
    CHECK (
        (event_kind = 'consumption' AND adjusts_usage_event_id IS NULL)
        OR (event_kind = 'adjustment_credit' AND adjusts_usage_event_id IS NOT NULL)
    ),
    FOREIGN KEY (tenant_id) REFERENCES saas_tenants(tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (workspace_id, tenant_id)
        REFERENCES saas_workspaces(workspace_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_id) REFERENCES saas_actors(actor_id) ON DELETE RESTRICT,
    FOREIGN KEY (adjusts_usage_event_id) REFERENCES saas_usage_events(usage_event_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, dimension, idempotency_key)
);

CREATE INDEX IF NOT EXISTS idx_saas_usage_tenant_dimension_time
    ON saas_usage_events(tenant_id, dimension, occurred_at);
CREATE INDEX IF NOT EXISTS idx_saas_usage_workspace_dimension_time
    ON saas_usage_events(workspace_id, dimension, occurred_at)
    WHERE workspace_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_saas_usage_adjustments
    ON saas_usage_events(adjusts_usage_event_id)
    WHERE adjusts_usage_event_id IS NOT NULL;

CREATE TRIGGER IF NOT EXISTS trg_saas_usage_adjustment_same_scope
BEFORE INSERT ON saas_usage_events
WHEN NEW.event_kind = 'adjustment_credit'
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1
        FROM saas_usage_events original
        WHERE original.usage_event_id = NEW.adjusts_usage_event_id
          AND original.tenant_id = NEW.tenant_id
          AND original.dimension = NEW.dimension
          AND (original.workspace_id = NEW.workspace_id
               OR (original.workspace_id IS NULL AND NEW.workspace_id IS NULL))
          AND original.event_kind = 'consumption'
    ) THEN RAISE(ABORT, 'usage adjustment must reference consumption in same tenant, workspace, and dimension') END;
END;

CREATE TABLE IF NOT EXISTS saas_quota_policies (
    policy_id               TEXT PRIMARY KEY CHECK (length(trim(policy_id)) > 0),
    tenant_id               TEXT NOT NULL,
    workspace_id            TEXT,
    dimension               TEXT NOT NULL CHECK (length(trim(dimension)) > 0 AND length(dimension) <= 128),
    limit_value             INTEGER NOT NULL CHECK (limit_value >= 0),
    window_kind             TEXT NOT NULL CHECK (
        window_kind IN ('lifetime', 'calendar_day', 'calendar_month', 'rolling', 'external_period')
    ),
    rolling_window_seconds  INTEGER CHECK (rolling_window_seconds IS NULL OR rolling_window_seconds > 0),
    external_start          TEXT,
    external_end            TEXT,
    limit_behavior          TEXT NOT NULL CHECK (limit_behavior IN ('hard', 'soft')),
    burst_allowance         INTEGER CHECK (burst_allowance IS NULL OR burst_allowance >= 0),
    warning_threshold       INTEGER CHECK (warning_threshold IS NULL OR warning_threshold >= 0),
    required_entitlement    TEXT CHECK (required_entitlement IS NULL OR length(trim(required_entitlement)) > 0),
    authority               TEXT NOT NULL CHECK (authority IN ('authoritative', 'local_provisional')),
    source_of_truth         TEXT NOT NULL CHECK (source_of_truth IN ('local', 'remote')),
    source_revision         INTEGER CHECK (source_revision IS NULL OR source_revision >= 0),
    stale_marked_at         TEXT,
    is_current              INTEGER NOT NULL DEFAULT 0 CHECK (is_current IN (0, 1)),
    created_at              TEXT NOT NULL CHECK (length(created_at) > 0),
    CHECK (
        (window_kind = 'rolling' AND rolling_window_seconds IS NOT NULL
            AND external_start IS NULL AND external_end IS NULL)
        OR (window_kind = 'external_period' AND rolling_window_seconds IS NULL
            AND external_start IS NOT NULL AND external_end IS NOT NULL)
        OR (window_kind IN ('lifetime', 'calendar_day', 'calendar_month')
            AND rolling_window_seconds IS NULL AND external_start IS NULL AND external_end IS NULL)
    ),
    FOREIGN KEY (tenant_id) REFERENCES saas_tenants(tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (workspace_id, tenant_id)
        REFERENCES saas_workspaces(workspace_id, tenant_id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_saas_quota_current_scope_dimension
    ON saas_quota_policies(tenant_id, IFNULL(workspace_id, ''), dimension)
    WHERE is_current = 1;
CREATE UNIQUE INDEX IF NOT EXISTS uq_saas_quota_remote_revision
    ON saas_quota_policies(tenant_id, IFNULL(workspace_id, ''), dimension, source_revision)
    WHERE source_revision IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_saas_quota_history
    ON saas_quota_policies(tenant_id, workspace_id, dimension, created_at DESC);

UPDATE schema_metadata
SET last_migration = 13,
    applied_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 1;
