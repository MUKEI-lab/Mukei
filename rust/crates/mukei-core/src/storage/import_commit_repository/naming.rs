fn resolve_display_name(
    transaction: &rusqlite::Transaction<'_>,
    scope_id: StorageScopeId,
    parent_node_id: StorageNodeId,
    admitted: &AllowedFileName,
    policy: DuplicatePolicy,
) -> std::result::Result<String, DbError> {
    if !name_exists(
        transaction,
        scope_id,
        parent_node_id,
        &admitted.normalized_name,
    )? {
        return Ok(admitted.display_name.clone());
    }

    match policy {
        DuplicatePolicy::RejectNameConflict => {
            Err(invariant("an active sibling already uses this filename"))
        }
        DuplicatePolicy::RenameNewEntry => {
            for index in 2..=MAX_CONFLICT_ATTEMPTS {
                let candidate = conflict_display_name(&admitted.display_name, index);
                if !name_exists(
                    transaction,
                    scope_id,
                    parent_node_id,
                    &candidate.to_ascii_lowercase(),
                )? {
                    return Ok(candidate);
                }
            }
            Err(invariant(
                "unable to allocate a unique filename within the bounded conflict limit",
            ))
        }
        DuplicatePolicy::ReplaceWithNewVersion => Err(invariant(
            "import replacement requires explicit copy-on-write versioning",
        )),
    }
}

fn name_exists(
    transaction: &rusqlite::Transaction<'_>,
    scope_id: StorageScopeId,
    parent_node_id: StorageNodeId,
    normalized_name: &str,
) -> std::result::Result<bool, DbError> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM storage_nodes \
             WHERE scope_id = ?1 AND parent_node_id = ?2 AND normalized_name = ?3 \
               AND state IN ('active', 'importing'))",
            rusqlite::params![
                scope_id.to_string(),
                parent_node_id.to_string(),
                normalized_name,
            ],
            |row| row.get::<_, i64>(0).map(|value| value != 0),
        )
        .map_err(DbError::from)
}

fn conflict_display_name(original: &str, index: u32) -> String {
    let suffix = format!(" ({index})");
    let extension_split = original
        .rfind('.')
        .filter(|position| *position > 0 && *position + 1 < original.len());

    let (stem, extension) = match extension_split {
        Some(position) => (&original[..position], Some(&original[position..])),
        None => (original, None),
    };
    let extension = extension.unwrap_or("");
    let reserved = suffix.len() + extension.len();
    let stem_budget = MAX_FILENAME_BYTES.saturating_sub(reserved);
    let stem = truncate_utf8(stem, stem_budget);
    format!("{stem}{suffix}{extension}")
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> &str {
    if value.len() <= maximum_bytes {
        return value;
    }
    let mut boundary = maximum_bytes.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &value[..boundary]
}

fn parse_scope_id(value: &str) -> std::result::Result<StorageScopeId, DbError> {
    Uuid::parse_str(value)
        .map(StorageScopeId)
        .map_err(|_| invariant("persisted storage scope id is invalid"))
}

fn parse_node_id(value: &str) -> std::result::Result<StorageNodeId, DbError> {
    Uuid::parse_str(value)
        .map(StorageNodeId)
        .map_err(|_| invariant("persisted storage node id is invalid"))
}

fn parse_workspace_id(value: &str) -> std::result::Result<WorkspaceId, DbError> {
    Uuid::parse_str(value)
        .map(WorkspaceId)
        .map_err(|_| invariant("persisted workspace id is invalid"))
}

fn parse_version_id(value: &str) -> std::result::Result<FileVersionId, DbError> {
    Uuid::parse_str(value)
        .map(FileVersionId)
        .map_err(|_| invariant("persisted file version id is invalid"))
}

fn parse_object_id(value: &str) -> std::result::Result<StorageObjectId, DbError> {
    Uuid::parse_str(value)
        .map(StorageObjectId)
        .map_err(|_| invariant("persisted storage object id is invalid"))
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn invariant(message: impl Into<String>) -> DbError {
    DbError::Domain(MukeiError::Invariant(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_suffix_preserves_the_last_extension() {
        assert_eq!(conflict_display_name("notes.md", 2), "notes (2).md");
        assert_eq!(
            conflict_display_name("archive.data.json", 3),
            "archive.data (3).json"
        );
    }

    #[test]
    fn exact_dot_names_are_treated_as_extensionless() {
        assert_eq!(conflict_display_name(".env", 2), ".env (2)");
        assert_eq!(conflict_display_name("README", 2), "README (2)");
    }

    #[test]
    fn conflict_names_remain_within_the_filename_byte_limit() {
        let original = format!("{}.md", "न".repeat(120));
        let candidate = conflict_display_name(&original, 10_000);
        assert!(candidate.len() <= MAX_FILENAME_BYTES);
        assert!(candidate.ends_with(" (10000).md"));
    }
}
