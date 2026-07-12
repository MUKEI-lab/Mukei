# QML Architecture Phase 2 Implementation

## Durable restoration

Migration `V011__ui_projection_sessions.sql` adds:

- `ui_session_state`
- `ui_drafts`

The records contain only presentation restoration metadata. Domain records remain authoritative in their existing repositories.

## Timeline projection

`MukeiTimelineModel` exposes stable Qt roles:

- `rowId`
- `type`
- `text`
- `phase`
- `kind`
- `status`
- `timestamp`
- `toolName`
- `parentId`
- `conversationId`
- `branchId`

Initial hydration replaces the model atomically. Older pages prepend by stable message ID and suppress duplicates. Streaming appends to one assistant row.

## Bridge snapshots

The agent bridge now exposes:

- `ui_session_json`
- `save_ui_session`
- `draft_json`
- `save_draft`
- `clear_draft`
- `conversation_list_json`
- `chat_snapshot_json`

Chat events carry both durable conversation and branch scope, allowing first-turn restoration without QML-generated identifiers.

## Recovery

`RecoveryStore` hydrates the interrupted turn before normal route restoration. The recovery page offers explicit resume, regenerate, or defer actions while preserving the partial response.
