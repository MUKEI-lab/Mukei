# SaaS Tenancy, Entitlements, and Usage Foundation

This note documents the local-first SaaS domain and persistence foundation now present in the merged source.

The original foundation was deliberately limited to `mukei-core` domain types, SQLite persistence, and migration registration. The current archive also contains a separate production-oriented transport boundary under `mukei-core::network::saas`. That transport module owns HTTP concerns such as endpoint validation, authentication injection, bounded retries/concurrency, cancellation, circuit state, and common JSON envelope parsing.

The merged source still does **not** claim a complete SaaS product. It does not yet prove finished identity-provider integration, server-side authorization, billing-provider integration, production endpoint wiring, QML SaaS flows, or retrofitted tenant/workspace ownership across every existing conversation/document table.

## Ownership model

- `tenant_id` is the billing/contract boundary.
- `workspace_id` is the collaboration and data-ownership boundary inside one tenant.
- `actor_id` identifies a human, service, or installation-local actor.
- Membership is a separate durable record. Revocation and suspension never require deleting actor identity.
- Public business identifiers are validated opaque strings; SQLite row IDs are not exposed as domain identity.
- Workspace ownership and membership tenant/workspace consistency are enforced with foreign keys. A workspace cannot exist without its tenant.

## Local-first behavior

`LocalScope::for_installation` derives deterministic local tenant, workspace, actor, and owner-membership identifiers from a stable installation identifier using independent domain separators. The resulting records are explicitly marked local and do not pretend to be cloud-issued identities.

`TenantWorkspaceRepository::ensure_local_scope` transactionally materializes those local records if absent. Existing conversation, document, recovery, and UI tables are unchanged and do not acquire new tenant/workspace foreign keys in this plan.

A future authenticated context can coexist with the local scope. No network connection is needed to create or use the local scope.

## Snapshot and revision invariants

Subscription and entitlement state are persisted as immutable historical snapshots with one current snapshot per tenant. Quota policies are also versioned and keep prior policy rows.

Repository application rules are deterministic:

1. A higher remote revision may replace the current record/snapshot.
2. A lower remote revision is ignored.
3. Reapplying the same immutable identity and content is idempotent.
4. Reusing one revision for different immutable content is a conflict.
5. When a source revision is absent, observation/synchronization time is the fallback ordering signal.
6. Authoritative state is not replaced by a local-provisional snapshot.
7. Stale state is marked in place without deleting historical rows. Reapplying the same authoritative snapshot may clear or refresh that stale marker without creating duplicate history.

No provider-specific billing object or secret is stored. Subscription state is intentionally provider-neutral.

## Entitlement decision safety

Entitlements are data-driven keys, not an `is_pro` flag. Decision methods distinguish:

- granted;
- denied;
- unknown/not present;
- stale or untrusted.

Local-provisional or stale entitlement state does not silently grant premium access. Subscription access resolution likewise distinguishes allowed, restricted, and stale/unknown states.

## Usage ledger invariants

Usage accounting is append-only.

- Every event has a non-negative quantity.
- The database uniquely enforces `(tenant_id, dimension, idempotency_key)`.
- Equivalent replays resolve to the already stored event.
- Reusing the same idempotency tuple for different mutation content is rejected.
- Corrections are separate `adjustment_credit` events that reference an original consumption event in the same tenant and dimension.
- Repository validation prevents cumulative credits from exceeding the original consumption quantity; aggregation also exposes checked net arithmetic rather than saturating underflow.
- Metadata is bounded, scalar-only, size-limited, and restricted to machine-safe keys/string tokens. Free-form prompts, document content, paths, secrets, and raw error payload categories are not accepted by the metadata API.

## Quota decisions

Quota policies are dimension-based and provider-neutral. Supported windows are lifetime, UTC calendar day, UTC calendar month, fixed rolling window, and externally supplied UTC period boundaries.

`QuotaPolicy::decide` is pure. It returns machine-readable states and figures for allowed, warning, hard-limit denial, entitlement denial, subscription restriction, and stale-authority uncertainty. Localized user-facing copy belongs in a later presentation layer.

## Future server synchronization boundary

A higher-level synchronization service may call the repository `apply_*` methods with server-issued tenant, workspace, actor, membership, subscription, entitlement, and quota records. The existing generic SaaS transport boundary can supply authentication/retry/circuit primitives, while a higher-level synchronization service remains responsible for concrete endpoint semantics and translating server payloads into the provider-neutral domain types.

The core repositories are responsible only for persistence invariants, idempotency, revision ordering, history retention, current-state resolution, and stale marking. They do not perform HTTP calls, token storage, billing-provider integration, telemetry, or UI updates.
