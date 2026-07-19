/// Process-local state for Temporary Chat sessions.
///
/// This type intentionally has no reference to `RuntimeProjectionStore`. Messages and
/// operation tokens held here are therefore structurally excluded from durable projection
/// writes. A process restart drops the entire value.
#[derive(Default)]
struct EphemeralChatState {
    lifecycle: Mutex<()>,
    conversations: RwLock<HashMap<(String, String), Vec<ChatMessage>>>,
    operations: Mutex<HashMap<String, EphemeralOperation>>,
    operation_ids: RwLock<HashMap<(String, String), HashSet<String>>>,
    retired: RwLock<HashMap<(String, String), ()>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EphemeralSessionState {
    Active,
    Retired,
    Absent,
}

struct EphemeralOperation {
    conversation_id: String,
    branch_id: String,
    token: CancellationToken,
}

impl EphemeralChatState {
    fn begin(&self, conversation_id: &str, branch_id: &str) -> bool {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let key = (conversation_id.to_owned(), branch_id.to_owned());
        if self
            .retired
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&key)
        {
            return false;
        }
        let mut conversations = self
            .conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if conversations.contains_key(&key) {
            return false;
        }
        conversations.insert(key.clone(), Vec::new());
        self.operation_ids
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, HashSet::new());
        true
    }

    fn session_state(&self, conversation_id: &str, branch_id: &str) -> EphemeralSessionState {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let key = (conversation_id.to_owned(), branch_id.to_owned());
        if self
            .conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&key)
        {
            EphemeralSessionState::Active
        } else if self
            .retired
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&key)
        {
            EphemeralSessionState::Retired
        } else {
            EphemeralSessionState::Absent
        }
    }

    fn is_registered(&self, conversation_id: &str, branch_id: &str) -> bool {
        self.session_state(conversation_id, branch_id) == EphemeralSessionState::Active
    }

    fn was_retired(&self, conversation_id: &str, branch_id: &str) -> bool {
        self.session_state(conversation_id, branch_id) == EphemeralSessionState::Retired
    }

    fn append_message(
        &self,
        conversation_id: &str,
        branch_id: &str,
        message: ChatMessage,
    ) -> bool {
        let mut conversations = self
            .conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(messages) = conversations.get_mut(&(
            conversation_id.to_owned(),
            branch_id.to_owned(),
        )) else {
            return false;
        };
        messages.push(message);
        true
    }

    fn history_if_registered(
        &self,
        conversation: ConversationId,
        branch: BranchId,
    ) -> Option<Vec<ChatMessage>> {
        self.conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(conversation.0.to_string(), branch.0.to_string()))
            .cloned()
    }

    fn clear_conversation(&self, conversation_id: &str, branch_id: &str) -> Option<usize> {
        let mut conversations = self
            .conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        conversations
            .get_mut(&(conversation_id.to_owned(), branch_id.to_owned()))
            .map(|messages| {
                let removed = messages.len();
                messages.clear();
                removed
            })
    }

    fn last_user_message(&self, conversation_id: &str, branch_id: &str) -> Option<ChatMessage> {
        self.conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(conversation_id.to_owned(), branch_id.to_owned()))
            .and_then(|messages| {
                messages
                    .iter()
                    .rev()
                    .find(|message| message.role == Role::User)
                    .cloned()
            })
    }

    fn remove_last_assistant(&self, conversation_id: &str, branch_id: &str) -> bool {
        let mut conversations = self
            .conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(messages) = conversations.get_mut(&(
            conversation_id.to_owned(),
            branch_id.to_owned(),
        )) else {
            return false;
        };
        let Some(index) = messages
            .iter()
            .rposition(|message| message.role == Role::Assistant)
        else {
            return false;
        };
        messages.remove(index);
        true
    }

    fn create_operation(
        &self,
        conversation_id: &str,
        branch_id: &str,
        proposed_operation_id: Option<&str>,
    ) -> Option<(String, CancellationToken)> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let key = (conversation_id.to_owned(), branch_id.to_owned());
        if !self
            .conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&key)
        {
            return None;
        }

        let operation_id = proposed_operation_id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let token = CancellationToken::new();
        let mut operation_ids = self
            .operation_ids
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let ids = operation_ids.get_mut(&key)?;
        let mut operations = self
            .operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ids.insert(operation_id.clone());
        operations.insert(
            operation_id.clone(),
            EphemeralOperation {
                conversation_id: conversation_id.to_owned(),
                branch_id: branch_id.to_owned(),
                token: token.clone(),
            },
        );
        Some((operation_id, token))
    }

    fn finish_operation(&self, operation_id: &str) {
        self.operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(operation_id);
        // Intentionally keep operation_id in the per-session history until end(),
        // so queued events from already-finished operations can still be scrubbed.
    }

    fn cancel_operation(&self, operation_id: &str) -> bool {
        let operation = self
            .operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(operation_id);
        let Some(operation) = operation else {
            return false;
        };
        operation.token.cancel();
        true
    }

    /// End a session and return every operation ID ever associated with it.
    fn end(&self, conversation_id: &str, branch_id: &str) -> Option<Vec<String>> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let key = (conversation_id.to_owned(), branch_id.to_owned());
        let removed = self
            .conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&key)
            .is_some();
        if !removed {
            return None;
        }
        self.retired
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key.clone(), ());

        let mut all_operation_ids = self
            .operation_ids
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&key)
            .unwrap_or_default();
        let mut operations = self
            .operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let active_operation_ids = operations
            .iter()
            .filter(|(_, operation)| {
                operation.conversation_id == conversation_id && operation.branch_id == branch_id
            })
            .map(|(operation_id, _)| operation_id.clone())
            .collect::<Vec<_>>();
        for operation_id in active_operation_ids {
            all_operation_ids.insert(operation_id.clone());
            if let Some(operation) = operations.remove(&operation_id) {
                operation.token.cancel();
            }
        }
        let mut operation_ids = all_operation_ids.into_iter().collect::<Vec<_>>();
        operation_ids.sort();
        Some(operation_ids)
    }

    fn cancel_all_and_clear(&self) {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut operations = self
            .operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (_, operation) in operations.drain() {
            operation.token.cancel();
        }
        self.conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.operation_ids
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.retired
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

#[cfg(test)]
mod ephemeral_chat_state_tests {
    use super::*;

    fn ids() -> (String, String, ConversationId, BranchId) {
        let conversation = Uuid::new_v4();
        let branch = Uuid::new_v4();
        (
            conversation.to_string(),
            branch.to_string(),
            ConversationId(conversation),
            BranchId(branch),
        )
    }

    #[test]
    fn temporary_history_is_process_local_and_purged_on_end() {
        let state = EphemeralChatState::default();
        let (conversation, branch, conversation_id, branch_id) = ids();
        assert!(state.begin(&conversation, &branch));
        assert!(!state.begin(&conversation, &branch));

        let message = ChatMessage::user_with_id(MessageId::new(), branch_id, "private");
        assert!(state.append_message(&conversation, &branch, message));
        assert_eq!(
            state
                .history_if_registered(conversation_id, branch_id)
                .expect("registered session")
                .len(),
            1
        );

        assert!(state.end(&conversation, &branch).is_some());
        assert!(state.was_retired(&conversation, &branch));
        assert!(!state.begin(&conversation, &branch));
        assert!(state
            .history_if_registered(conversation_id, branch_id)
            .is_none());
    }

    #[test]
    fn ending_temporary_chat_cancels_only_its_operations() {
        let state = EphemeralChatState::default();
        let (conversation_a, branch_a, _, _) = ids();
        let (conversation_b, branch_b, _, _) = ids();
        assert!(state.begin(&conversation_a, &branch_a));
        assert!(state.begin(&conversation_b, &branch_b));

        let (operation_a, token_a) = state
            .create_operation(&conversation_a, &branch_a, None)
            .expect("operation a");
        let (operation_b, token_b) = state
            .create_operation(&conversation_b, &branch_b, None)
            .expect("operation b");
        assert!(!token_a.is_cancelled());
        assert!(!token_b.is_cancelled());

        let ended_operations = state
            .end(&conversation_a, &branch_a)
            .expect("temporary session");
        assert_eq!(ended_operations, vec![operation_a.clone()]);
        assert!(token_a.is_cancelled());
        assert!(!token_b.is_cancelled());
        assert!(!state.cancel_operation(&operation_a));
        assert!(state.cancel_operation(&operation_b));
        assert!(token_b.is_cancelled());
    }

    #[test]
    fn finished_operation_id_is_retained_until_session_end() {
        let state = EphemeralChatState::default();
        let (conversation, branch, _, _) = ids();
        assert!(state.begin(&conversation, &branch));
        let (operation_id, _token) = state
            .create_operation(&conversation, &branch, None)
            .expect("operation");
        state.finish_operation(&operation_id);

        let ended_operations = state
            .end(&conversation, &branch)
            .expect("temporary session");
        assert_eq!(ended_operations, vec![operation_id]);
    }

    #[test]
    fn operation_creation_fails_after_session_end() {
        let state = EphemeralChatState::default();
        let (conversation, branch, _, _) = ids();
        assert!(state.begin(&conversation, &branch));
        assert!(state.end(&conversation, &branch).is_some());
        assert!(state.create_operation(&conversation, &branch, None).is_none());
    }
}
