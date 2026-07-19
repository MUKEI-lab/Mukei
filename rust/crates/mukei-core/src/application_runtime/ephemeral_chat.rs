/// Process-local state for Temporary Chat sessions.
///
/// This type intentionally has no reference to `RuntimeProjectionStore`. Messages and
/// operation tokens held here are therefore structurally excluded from durable projection
/// writes. A process restart drops the entire value.
#[derive(Default)]
struct EphemeralChatState {
    conversations: RwLock<HashMap<(String, String), Vec<ChatMessage>>>,
    operations: Mutex<HashMap<String, EphemeralOperation>>,
    retired: RwLock<HashMap<(String, String), ()>>,
}

struct EphemeralOperation {
    conversation_id: String,
    branch_id: String,
    token: CancellationToken,
}

impl EphemeralChatState {
    fn begin(&self, conversation_id: &str, branch_id: &str) -> bool {
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
        conversations.insert(key, Vec::new());
        true
    }

    fn is_registered(&self, conversation_id: &str, branch_id: &str) -> bool {
        self.conversations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&(conversation_id.to_owned(), branch_id.to_owned()))
    }

    fn was_retired(&self, conversation_id: &str, branch_id: &str) -> bool {
        self.retired
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&(conversation_id.to_owned(), branch_id.to_owned()))
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
    ) -> (String, CancellationToken) {
        let operation_id = proposed_operation_id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let token = CancellationToken::new();
        self.operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                operation_id.clone(),
                EphemeralOperation {
                    conversation_id: conversation_id.to_owned(),
                    branch_id: branch_id.to_owned(),
                    token: token.clone(),
                },
            );
        (operation_id, token)
    }

    fn finish_operation(&self, operation_id: &str) {
        self.operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(operation_id);
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

    fn end(&self, conversation_id: &str, branch_id: &str) -> bool {
        let key = (conversation_id.to_owned(), branch_id.to_owned());
        let removed = self
            .conversations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&key)
            .is_some();
        if !removed {
            return false;
        }
        self.retired
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, ());

        let mut operations = self
            .operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let operation_ids = operations
            .iter()
            .filter(|(_, operation)| {
                operation.conversation_id == conversation_id && operation.branch_id == branch_id
            })
            .map(|(operation_id, _)| operation_id.clone())
            .collect::<Vec<_>>();
        for operation_id in operation_ids {
            if let Some(operation) = operations.remove(&operation_id) {
                operation.token.cancel();
            }
        }
        true
    }

    fn cancel_all_and_clear(&self) {
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

        assert!(state.end(&conversation, &branch));
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

        let (operation_a, token_a) = state.create_operation(&conversation_a, &branch_a, None);
        let (operation_b, token_b) = state.create_operation(&conversation_b, &branch_b, None);
        assert!(!token_a.is_cancelled());
        assert!(!token_b.is_cancelled());

        assert!(state.end(&conversation_a, &branch_a));
        assert!(token_a.is_cancelled());
        assert!(!token_b.is_cancelled());
        assert!(!state.cancel_operation(&operation_a));
        assert!(state.cancel_operation(&operation_b));
        assert!(token_b.is_cancelled());
    }
}
