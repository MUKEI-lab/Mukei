# 07 — Storage, Workspace, Project, and Artifact Model

Status: **Draft v0.1**

This document defines the product/domain model required by the UI/UX blueprint for durable local files and structured work.

It incorporates useful primitives from `temp/universal-storage-workspace-v0.1` without treating that experimental branch as automatically canonical.

## Core principle

The user-facing concepts below must remain distinct:

```text
Universal Storage ≠ Workspace ≠ Project ≠ Chat ≠ Artifact
```

They may reference the same durable file bytes, but they have different product semantics and lifecycle rules.

---

# 1. Product concepts

## Universal Storage

App-wide durable library of files Mukei can access or has produced.

Use cases:

- imported user files not tied exclusively to one task;
- generated documents/exports saved for later;
- reusable assets;
- user-visible storage browsing.

Universal Storage is **not** the Android filesystem root and should not expose implementation paths.

## Workspace

An isolated working set for structured task execution.

Use cases:

- code project generation;
- multi-file editing;
- document/research bundle;
- generated working files;
- intermediate outputs;
- final artifacts.

A workspace is operational and task-oriented. It can be surfaced inline in Conversation and as a full Workspace screen.

## Project

A long-lived organizational/context layer.

Recommended product interpretation:

> A Project aggregates and contextualizes chats, workspaces, files, and artifacts without requiring duplicate physical file storage.

Exact ownership/cardinality remains an ADR decision.

## Chat / Conversation

The conversational interaction history and intent/decision surface.

A chat may initiate or reference structured work, but chat history itself is not the storage hierarchy.

## Artifact

A user-meaningful deliverable produced by work.

Examples:

- ZIP;
- report;
- PDF;
- generated image;
- spreadsheet;
- code bundle;
- final document.

Recommended data-model rule:

> Artifact is a semantic/provenance projection over one or more durable storage file versions, not a second copy of file bytes by default.

---

# 2. Layered storage model

Recommended architecture:

```text
User-facing logical node
(StorageNode)
        ↓ current version
FileVersion
        ↓ immutable content
StorageObject
        ↓ encrypted bytes
Object store filesystem
```

This separation is strong and should be retained from the temp prototype.

## StorageObject

Represents immutable encrypted content bytes.

Properties should include:

- opaque object ID;
- plaintext hash/size metadata as safely stored in encrypted DB;
- encrypted relative object path;
- detected format/MIME;
- encryption version;
- integrity state;
- creation/verification timestamps.

Object path must never be derived from untrusted user filenames.

## FileVersion

Represents one immutable logical version referencing a StorageObject.

Properties:

- version ID;
- object ID;
- previous version ID where applicable;
- monotonically meaningful version number within logical file history;
- provenance/creator type;
- original filename metadata;
- encoding/language metadata;
- timestamp.

## StorageNode

Represents logical user-visible directory/file placement.

Properties:

- node ID;
- storage scope ID;
- parent node ID;
- file/directory kind;
- display name;
- normalized collision key;
- current version ID for files;
- optional system role;
- lifecycle state;
- timestamps.

Logical rename/move should not require rewriting immutable object bytes.

---

# 3. Storage scopes

Every logical node belongs to exactly one storage scope.

## Scope types

```text
Universal
Workspace
```

Potential future scope types should require explicit product/ADR justification.

## Universal scope

There is one active app-wide universal scope.

It owns the logical root for Universal Storage.

## Workspace scope

Each workspace owns one isolated storage scope.

All workspace file hierarchy references must remain inside that scope.

## Isolation invariant

A node in workspace A MUST NOT:

- use a parent from workspace B;
- target an import directory in workspace B;
- mutate another workspace through stale/guessed IDs;
- become accessible merely because another chat/project knows its filename.

Isolation must be enforced in both domain authorization and persistence/database constraints.

---

# 4. Workspace cardinality decision

The temp prototype currently hardcodes:

```text
one chat owns exactly one workspace
```

and its database enforces one non-deleted workspace scope per `owner_chat_id`.

This is useful as a simple Phase-1 model, but the product specification should **not lock it implicitly** before ADR review.

## Questions to resolve

1. Can one chat create multiple independent workspaces over time?
2. Can a workspace be created from a Project before any chat exists?
3. Can multiple chats collaborate on/reference one workspace?
4. Does branching a chat share or fork workspace state?
5. What happens when a chat is deleted but workspace files must remain?

## Recommended protocol/data rule now

Even if v0.1 ships one-workspace-per-chat:

- workspace must have its own stable `workspace_id`;
- commands must target explicit workspace/scope identities;
- UI/backend code must not derive workspace identity only from `chat_id` forever.

This preserves migration flexibility.

---

# 5. Recommended workspace system structure

Temp prototype defines:

```text
Workspace root
├── Uploaded files
├── Generated files
├── Drafts
├── Research
├── Exports
├── Temporary
└── Trash
```

These roles are useful backend/domain invariants.

## UX rule

System directories are **not required to appear as literal folders in every UI**.

For example:

- Activity may write into Generated/Research internally;
- Workspace screen may present a cleaner semantic grouping;
- Temporary should generally remain hidden;
- Trash can be a dedicated view/action rather than a permanent folder row.

Domain role and presentation hierarchy are separate.

## System-role rules

- system directories are not user-deletable individually;
- exactly one active directory per required role per scope;
- role identities should be stable and machine-readable;
- display labels may be localized.

---

# 6. Universal Storage structure

Universal Storage should remain simpler than workspaces.

Minimum required concepts:

- root;
- user-created/imported folders where supported;
- durable files;
- generated/exported items;
- Trash.

Do not automatically mirror every workspace system directory into Universal Storage.

Universal Storage is a durable library, not a task execution scratch tree.

---

# 7. File provenance

Every durable file/version should carry provenance sufficient to answer:

- Where did this come from?
- Was it imported or generated?
- Which chat/workspace/project produced it?
- Which version is current?
- Did it leave the device/export?

Recommended provenance categories based on prototype concepts:

```text
user_import
user_edit
assistant_generation
research
system_recovery
```

Additional structured links may include:

```text
source_chat_id?
source_operation_id?
source_workspace_id?
source_project_id?
```

Provenance links should not create ownership ambiguity.

---

# 8. Import target model

An import must target an explicit destination.

Product targets:

```text
Universal Storage directory
Workspace uploaded-files role
Workspace explicit directory
```

The temp prototype already models these three target shapes conceptually.

## Composer attachment behavior

When user attaches a file during chat, product policy must choose explicitly:

- temporary attachment only;
- durable import into current workspace;
- durable import into Universal Storage;
- ask user when ambiguity matters.

The UI MUST NOT silently create permanent duplicates without defined policy.

Recommended default for structured chat/workspace flow:

> Durable import into the active workspace's Uploaded Files role, with clear local-storage semantics.

For generic Home attachments before a workspace exists, policy requires ADR/product decision.

---

# 9. Import transaction lifecycle

Import is a durable transaction, not one synchronous file-copy call.

Recommended states adapted from prototype:

```text
created
validating
copying/staging
hashing
encrypting
committing_object
committing_node
indexing
completed
cancel_requested
cancelled
failed
recovering
```

UI may collapse these into simpler human states while retaining machine accuracy.

## Invariants

- validation occurs before publication;
- untrusted filename never determines filesystem path;
- file size/type/policy limits are enforced;
- plaintext staging is app-private and bounded;
- encrypted object publication and DB commit are crash-recoverable;
- a durable stored file can exist even if indexing later fails;
- recovery can distinguish orphan staging/object/database states.

---

# 10. File admission policy

File policy should be versioned and centralized.

It should define:

- maximum import size by product capability;
- allowed/unsupported types;
- filename normalization/sanitization;
- extension/MIME consistency policy;
- text encoding validation;
- duplicate-name behavior;
- executable/archive handling policy;
- decompression/archive safety when supported.

UI copy must map machine policy errors into human explanations.

Do not duplicate policy logic in Compose.

---

# 11. Duplicate-name and version policy

Prototype supports conceptual policies:

```text
Rename new entry
Reject conflict
Replace with new version
```

Recommended product default:

- importing a different file with same name → rename new entry (`name (2).ext`) unless user explicitly chooses replace;
- replace is an explicit version-creating action;
- never silently overwrite immutable history.

## Version semantics

New versions should reference immutable new StorageObjects while retaining previous version chain.

Trash/delete of a logical node does not immediately imply object deletion if another version/node still references the object.

---

# 12. Encryption model

At-rest security requirements:

1. metadata/database remains SQLCipher-encrypted;
2. object/file bytes remain separately encrypted in the object store;
3. raw object encryption key material is not stored plaintext;
4. Android Keystore protects/wraps persistent key material;
5. database key and object-store key SHOULD be separate cryptographic domains;
6. plaintext staging is temporary, app-private, and cleaned/recovered deterministically;
7. encrypted object integrity is verified.

The temp prototype's separate object-store key direction is preferred over reusing the SQLCipher key for file encryption.

## Key loss/failure

Failure to unwrap a key must fail closed.

The product must not silently regenerate a replacement key if doing so would orphan existing encrypted data.

---

# 13. Object store publication model

Object store bytes are immutable after successful publication.

Recommended path:

```text
validated plaintext/staged source
  ↓ hash + size
bounded encryption
  ↓ temporary encrypted object
fsync/atomic publish
  ↓
storage_objects metadata commit
  ↓
file_version commit
  ↓
storage_node current version update
```

Exact transaction ordering must be designed with the operation journal/recovery model.

## Invariants

- no partially encrypted object becomes visible as verified;
- no logical node points to nonexistent object/version after committed success;
- orphan recovery is deterministic;
- object filenames/paths are opaque IDs.

---

# 14. Integrity state

Recommended object integrity states:

```text
pending
verified
corrupt
missing
quarantined
```

User-facing UI normally maps these into simpler states.

Corrupt/missing objects must fail closed and must not be treated as readable files.

Recovery may quarantine suspicious objects instead of deleting evidence automatically.

---

# 15. Indexing / document intelligence

Storage and indexing are separate capabilities.

Recommended pipeline:

```text
Durable file version
  ↓ optional parser/index job
pending → parsing → chunking → embedding → ready
                                 ├→ failed
                                 ├→ cancelled
                                 └→ stale
```

## Critical invariant

```text
File stored successfully ≠ File indexed successfully
```

UI must represent `Stored but indexing failed` truthfully.

Deleting/replacing versions must invalidate/reconcile indexes according to policy.

---

# 16. Trash model

Trash is a logical lifecycle state, not immediate physical erasure.

Recommended node states:

```text
active
importing
trashed
quarantined
deleting
deleted
```

## Trash behavior

- trash records original logical location;
- restore returns to original or conflict-resolved location;
- permanent deletion is separate/destructive;
- system-role roots are protected;
- cross-scope restore is not implicit.

## Object garbage collection

Permanent node/version deletion may leave unreferenced encrypted objects.

Physical object deletion should use a safe reference-aware garbage-collection/reclamation process, not ad-hoc delete-on-node-removal.

---

# 17. Workspace deletion

Workspace deletion semantics depend on Project/Chat ownership ADR.

At minimum confirmation must state:

- what workspace/files will be removed from Mukei;
- whether linked chat remains;
- whether Project remains;
- whether exported copies outside Mukei remain;
- whether shared/referenced Universal Storage items are untouched.

No destructive cascade should be hidden from the user.

---

# 18. Chat deletion interaction

Recommended default principle:

> Deleting a chat must not automatically destroy independently durable files/artifacts without explicit scope confirmation.

If v0.1 workspace is strictly owned by chat, deleting chat should still present a choice or clear combined deletion contract rather than silently cascade user work.

This is a key ADR topic.

---

# 19. Project model recommendation

Recommended v0.1 direction:

```text
Project
├── references Chats
├── references Workspaces
├── references/organizes Storage Nodes or Artifacts
└── stores project-specific metadata/context
```

Project should **not** duplicate object bytes simply because a file is added to a project.

## Why aggregation is preferred

- avoids duplicate encrypted files;
- allows one durable artifact to appear in Storage and Project;
- separates organization from storage ownership;
- makes chat deletion less destructive;
- supports future multi-chat projects.

This recommendation should be locked by ADR before implementation.

---

# 20. Artifact model recommendation

Artifact should be modeled as semantic metadata referencing backing storage identity.

Candidate structure:

```text
Artifact
- artifact_id
- kind
- title/display_name
- backing_node_id or bundle manifest
- backing_version_id(s)
- source_workspace_id?
- source_operation_id?
- source_chat_id?
- project_id?
- readiness
- created_at
- export metadata/history?
```

## Artifact kinds

Examples:

```text
file
bundle/zip
report
document
image
spreadsheet
dataset
code_project
```

Exact enum should be extensible/versioned.

## Rule

An ordinary generated file may become an Artifact when it is designated as a user-meaningful deliverable.

Not every intermediate generated file is an Artifact.

---

# 21. WorkspaceCard projection

The inline conversation WorkspaceCard should not query raw filesystem hierarchy directly.

Recommended projection:

```text
WorkspaceSummary
- workspace_id
- title
- state
- created_count
- edited_count
- failed_count
- artifact_count
- active_phase?
- can_view
- can_export
```

Counts should derive from authoritative workspace/activity data.

---

# 22. Storage screen projection

Storage list/query requires bounded/paged projection:

```text
StorageItemSummary
- node_id
- scope_id
- display_name
- kind
- mime/type
- size?
- state
- provenance
- modified_at
- thumbnail/preview capability?
- artifact_role?
- available_actions
```

Do not expose encrypted object paths or internal staging paths.

---

# 23. File preview/open contract

Preview must be capability-driven.

Examples:

- text/code → bounded decoded preview;
- image → safe local content handle;
- PDF/document → supported renderer or metadata/open action;
- archive → manifest/metadata rather than unsafe automatic extraction.

Large file bytes should not be serialized through Protocol JSON.

Use controlled file descriptors/content handles/native readers according to platform architecture.

---

# 24. Export semantics

Export creates/copies data outside the internal encrypted workspace boundary.

The UI should disclose this naturally.

## Invariants

- internal copy remains unless user separately deletes it;
- export destination is explicit/system-mediated;
- export success is recorded only after write completion where possible;
- external file is outside Mukei's future encryption/control guarantee;
- repeated export is allowed while backing artifact exists.

---

# 25. Scope authorization

Every mutation must validate:

```text
requested actor/context
requested scope
requested node/version/workspace
allowed relationship
```

Do not authorize solely by possession of opaque ID if context policy requires workspace/chat/project ownership.

Temp prototype's `WorkspaceAccessContext` and same-scope database guards are good defense-in-depth patterns.

---

# 26. Database invariants worth preserving from prototype

The temp migrations establish several strong constraints that should be retained or deliberately superseded:

- exactly one active universal scope;
- unique workspace IDs;
- logical nodes reference scope;
- active sibling names are unique within parent/scope;
- one system role per scope;
- file nodes reference immutable current versions;
- imports reference explicit target scope + directory;
- parent directory must belong to same scope;
- import target must belong to same scope;
- root node identity is unique;
- operation journal supports crash recovery.

The **one workspace per chat** unique constraint remains pending ADR rather than automatically accepted.

---

# 27. Operation journal

Filesystem + database mutations need durable recovery phases.

Prototype states:

```text
prepared
applied_filesystem
applied_database
committed
rolling_back
rolled_back
recovery_required
```

This pattern is appropriate for:

- imports;
- object publication;
- move/rename with filesystem effects;
- export preparation where internal durable state changes;
- permanent deletion/reclamation.

UI normally sees a simplified recovery state, not journal internals.

---

# 28. Universal Storage vs Workspace copy/reference policy

When moving an item between Universal Storage and Workspace, product should distinguish:

## Reference/share logical content

Same immutable StorageObject/FileVersion can be referenced by a new logical node where policy permits.

Pros:

- no byte duplication;
- efficient.

Requires clear lifecycle/reference counting.

## Copy as new logical file/version

Creates independent logical history, potentially deduplicating underlying immutable object by content hash.

Pros:

- clearer independence.

Recommendation: default semantics should be explicit in UI language (`Add to workspace`, `Copy`, `Move`) and backend action; do not silently blur them.

---

# 29. Security boundaries

Internal paths must remain app-private.

Never expose to UI/domain as stable identity:

- raw app-private filesystem paths;
- staging paths;
- object-store relative paths;
- encryption key aliases unless diagnostics require redacted metadata.

Stable IDs are the contract.

---

# 30. Minimum v0.1 storage slice

For M3/M4, implement at minimum:

## Domain

- one Universal Storage scope;
- explicit Workspace IDs + isolated scopes;
- StorageNode/FileVersion/StorageObject model;
- encrypted object store;
- versioned file admission policy;
- staged import transaction + recovery;
- trash/restore basics;
- bounded listing/query.

## Android UX

- import from system picker;
- Storage list;
- workspace uploaded/generated file listing;
- import/storage/indexing state separation;
- file metadata/preview for supported types;
- clear local-storage trust labels.

## Defer if necessary

- arbitrary move/copy across scopes;
- complex version-history UI;
- advanced garbage collection UI;
- multi-workspace project orchestration;
- collaboration/cloud sync.

Security/recovery internals must not be deferred if data durability depends on them.

---

# 31. Decisions requiring ADR

## ADR-A — Workspace cardinality/ownership

Options:

- exactly one workspace per chat;
- multiple workspaces per chat;
- workspace independent, chats reference it.

## ADR-B — Project ownership

Recommended: aggregation/reference layer rather than primary byte-owning storage scope.

## ADR-C — Artifact identity

Recommended: semantic durable projection referencing storage versions.

## ADR-D — Cross-scope copy/reference semantics

Define Move vs Copy vs Add/reference and deletion consequences.

## ADR-E — Object key lifecycle

Define separate object-store encryption key rotation/recovery policy.

---

# 32. Product invariants summary

The implementation is acceptable only if all are true:

1. User filenames never become unsafe filesystem paths.
2. File bytes are encrypted at rest in object store.
3. Database and object-store cryptographic domains are deliberately managed.
4. Workspaces are isolated by explicit scope IDs and fail-closed checks.
5. Durable stored file state is distinct from indexing state.
6. File history uses immutable versions rather than silent overwrite.
7. Partial/crashed imports recover deterministically.
8. Trash is separate from permanent deletion.
9. Project organization does not silently duplicate/delete underlying data.
10. Artifact/export semantics tell the user what remains inside Mukei and what leaves it.
11. Stable opaque IDs, not filesystem paths, cross UI/backend contracts.
12. One-workspace-per-chat is not permanently frozen without ADR approval.
