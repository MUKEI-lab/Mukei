# ADR-008: Temporary Chat ephemeral isolation

- **Status:** Proposed
- **Decision area:** Conversation persistence, privacy, RAG/context isolation, Android UX
- **Priority:** Critical before enabling Temporary Chat

## 1. Context

Mukei needs a first-class **Temporary Chat** mode for work that should not become durable chat history. The experience must still support normal in-session reasoning, context-window behavior, file/RAG grounding, and tool activity, but none of that chat-local state may leak into later conversations.

A UI-only “incognito” toggle is insufficient. The privacy promise must be enforced at the runtime/storage boundary so process death, navigation, retries, background work, and RAG indexing cannot accidentally persist or reuse temporary-session state.

## 2. Decision

Introduce an explicit conversation persistence policy:

```text
ConversationPersistencePolicy
├── Durable
└── Ephemeral
```

A Temporary Chat uses `Ephemeral` policy and MUST satisfy all of the following:

1. **No durable conversation projection.** Messages, title, branch history, summaries, and chat metadata MUST NOT be written to the durable conversation store.
2. **Session-scoped identity.** Each Temporary Chat receives a unique ephemeral conversation/session identifier that is never reused by another chat.
3. **Normal in-session context.** Context-window construction, tool results, citations, and assistant reasoning inputs MAY use prior turns from the same temporary session while it is alive.
4. **RAG isolation.** Chat-local retrieval state, temporary embeddings/indexes, retrieval caches, and attachment-derived context MUST be scoped to that ephemeral session and MUST NOT become eligible context for other chats.
5. **No cross-chat memory transfer.** Temporary-chat content MUST NOT update durable conversation memory, long-term memory, project memory, personalization memory, or any shared retrieval corpus unless the user performs an explicit save/export action whose consequences are separately disclosed.
6. **Lifecycle deletion.** Leaving/closing the Temporary Chat, starting another Temporary Chat, process death, runtime restart, or app restart MUST make the temporary conversation unrecoverable as chat history. Any temporary files/indexes created only for that session MUST be deleted or made unreachable and cleaned on startup.
7. **No Chats-index entry.** Temporary Chats MUST NOT appear in recent chats, pinned/starred chats, search, project chat lists, or history.
8. **Explicit durable-file boundary.** Existing durable files may be referenced when the user explicitly grants/attaches them. Referencing a durable file does not make the temporary chat durable. New generated artifacts MUST require an explicit save/export action before entering durable storage.
9. **Fail closed.** If runtime isolation or cleanup cannot be guaranteed, Temporary Chat creation MUST be unavailable rather than silently falling back to durable persistence.

## 3. UI contract

On the Home/top-level screen:

- the primary compose-pencil action remains **New chat**;
- the secondary action becomes a dedicated **Temporary Chat** icon/affordance;
- a generic top-level `⋮` menu is not used for navigation shortcuts.

Inside an existing durable chat, a contextual overflow menu MAY expose chat-specific actions such as:

- Share
- Rename
- Star / Unstar
- Add to project
- Delete

Temporary Chat MUST show a persistent but quiet visual indicator while active so the user can distinguish it from a saved chat.

## 4. Alternatives considered

### A. UI-only incognito toggle
Rejected. It can misrepresent privacy if the runtime still writes normal projections or shared RAG state.

### B. Create a normal durable chat and delete it on exit
Rejected. Temporary content could survive crashes/process death, enter indexes/backups, or leak through shared memory before deletion.

### C. Store encrypted temporary chats and hide them from history
Rejected for the initial contract. “Temporary” should mean non-durable, not merely hidden.

## 5. Consequences

- Conversation persistence APIs need an explicit persistence policy rather than implicit persistence.
- Projection writes must be bypassed for ephemeral sessions.
- RAG/index/cache ownership needs an ephemeral session scope and cleanup path.
- Process-death recovery must intentionally *not* restore Temporary Chats.
- Tests must prove that temporary-session content does not appear in durable projections or later-chat retrieval.

## 6. Migration / compatibility impact

Additive for new conversations. Existing durable conversations remain unchanged. Protocol changes SHOULD remain backward-compatible and explicit; older clients that do not understand ephemeral policy must not be allowed to create Temporary Chats.

## 7. Security / privacy impact

This is a privacy boundary, not only a UX mode. Required tests include:

- no durable conversation write for ephemeral sessions;
- no temporary message in chat index/history after runtime restart;
- no temporary RAG/embedding retrieval from a different conversation;
- cleanup of temporary session artifacts after exit/process death;
- explicit-save path is the only route from ephemeral generated output to durable storage.

## 8. Product / UX impact

Temporary Chat gives users a clear mental model:

> Work normally for this session, but do not keep or reuse this chat afterward.

The icon and copy must communicate **temporary/private session**, not generic settings or overflow navigation.

## 9. Implementation sequence

1. Define typed persistence policy and ephemeral session identity.
2. Add runtime tests proving no durable projection writes.
3. Add session-scoped RAG/context ownership and cleanup tests.
4. Expose the capability through Protocol V2 / Kotlin boundary.
5. Only then enable the Temporary Chat icon and active-state UI.

Until steps 1–4 are green, the product MUST NOT present a clickable Temporary Chat control that implies these guarantees.
