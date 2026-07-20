# ADR-004 — Artifact Identity and Lifecycle

Status: **Proposed**  
Priority: **High before Artifact/export implementation**

## Context

The UI treats artifacts as meaningful deliverables: ZIPs, reports, PDFs, images, spreadsheets, datasets, or code bundles.

Not every generated intermediate file should become an Artifact, and duplicating file bytes merely to create an artifact identity would complicate storage/lifecycle.

## Proposed decision

**Artifact is a first-class semantic metadata entity that references one or more durable storage file versions or a bundle manifest. It does not duplicate backing bytes by default.**

Candidate fields:

```text
artifact_id
kind
title/display_name
backing_node_id? / manifest
backing_version_id(s)
source_workspace_id?
source_operation_id?
source_chat_id?
project_id?
readiness
created_at
export metadata/history?
```

## Alternatives considered

### A. Artifact = any generated file

Rejected because intermediate files would clutter product semantics and completion UX.

### B. Artifact is a separate physical copy

Rejected as default because it wastes storage and creates divergence from workspace file versions.

### C. Semantic projection over durable storage — proposed

Preserves stable deliverable identity while reusing canonical storage.

## Lifecycle

Recommended states:

```text
generating
ready
exporting
ready_with_export
export_failed (internal artifact still ready)
deleted/missing
```

Export state is separate from artifact generation state.

## Consequences

- an ArtifactCard queries artifact metadata + backing storage;
- export/share does not remove internal artifact;
- re-export is possible while backing versions exist;
- deleting backing content must account for artifact references;
- artifact kind is extensible/versioned.

## Security / privacy impact

External export leaves Mukei's encrypted internal boundary. UI must communicate this naturally.

Artifact metadata must not expose internal object-store paths.

## Product / UX impact

`Your files are ready` refers to a stable deliverable object, not merely a transient operation event.
