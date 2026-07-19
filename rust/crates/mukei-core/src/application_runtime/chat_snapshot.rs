impl FeatureState {
    fn snapshot_with_conversations(&self, platform: PlatformBrokerSnapshot) -> Value {
        let mut snapshot = self.snapshot(platform);
        if let Some(object) = snapshot.as_object_mut() {
            let branches = self
                .conversations_snapshot()
                .get("branches")
                .cloned()
                .unwrap_or_else(|| json!([]));
            object.insert("conversation_branches".to_owned(), branches);
        }
        snapshot
    }
}
