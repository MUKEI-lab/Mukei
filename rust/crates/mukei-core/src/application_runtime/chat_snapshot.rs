impl FeatureState {
    fn snapshot_with_conversations(&self, platform: PlatformBrokerSnapshot) -> Value {
        let mut snapshot = self.snapshot(platform);
        if let Some(object) = snapshot.as_object_mut() {
            let conversations = self.conversations_snapshot();
            let branches = conversations
                .get("branches")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let metadata = conversations
                .get("conversations")
                .cloned()
                .unwrap_or_else(|| json!([]));
            object.insert("conversation_branches".to_owned(), branches);
            object.insert("conversations".to_owned(), metadata);
        }
        snapshot
    }
}
