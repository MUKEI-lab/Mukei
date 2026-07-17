async fn load_committed_receipt(
    pool: &DatabasePool,
    transaction_id: ImportTransactionId,
) -> Result<Option<ImportCommitReceipt>> {
    pool.with_conn(move |connection| {
        let row: Option<(String, String, String, String, String, String, String)> = connection
            .query_row(
                "SELECT j.journal_id, j.scope_id, n.parent_node_id, n.node_id, \
                        n.current_version_id, fv.object_id, n.display_name \
                 FROM operation_journal j \
                 JOIN storage_nodes n ON n.node_id = j.node_id \
                 JOIN file_versions fv ON fv.version_id = n.current_version_id \
                 WHERE j.transaction_id = ?1 AND j.operation_type = ?2 \
                   AND j.state = 'committed' \
                 ORDER BY j.created_at DESC LIMIT 1",
                rusqlite::params![transaction_id.to_string(), OPERATION_TYPE],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()?;

        row.map(|row| {
            Ok(ImportCommitReceipt {
                journal_id: row.0,
                scope_id: parse_scope_id(&row.1)?,
                parent_node_id: parse_node_id(&row.2)?,
                node_id: parse_node_id(&row.3)?,
                version_id: parse_version_id(&row.4)?,
                object_id: parse_object_id(&row.5)?,
                display_name: row.6,
                reused_object: true,
                reused_version: true,
            })
        })
        .transpose()
    })
    .await
}

async fn prepare_filesystem_journal(
    pool: &DatabasePool,
    request: ImportCommitRequest,
) -> Result<PreparedImport> {
    pool.with_conn(move |connection| {
        let transaction =
            connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (scope_id, parent_node_id) = validate_import_target(&transaction, &request)?;

        let existing: Option<(String, String, String)> = transaction
            .query_row(
                "SELECT journal_id, state, payload_json FROM operation_journal \
                 WHERE transaction_id = ?1 AND operation_type = ?2 \
                 ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![request.transaction_id.to_string(), OPERATION_TYPE],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let journal_id = match existing {
            Some((journal_id, state, payload)) => {
                if state == "committed" {
                    return Err(invariant(
                        "committed import journal exists without a readable file receipt",
                    ));
                }
                if !matches!(
                    state.as_str(),
                    "prepared" | "applied_filesystem" | "recovery_required"
                ) {
                    return Err(invariant(format!(
                        "import journal is in an unsupported recovery state: {state}"
                    )));
                }
                validate_journal_payload(&payload, &request)?;
                journal_id
            }
            None => {
                let journal_id = Uuid::new_v4().to_string();
                let relative_path = request
                    .stored_object
                    .relative_path
                    .to_str()
                    .ok_or_else(|| invariant("object relative path is not UTF-8"))?;
                let payload = json!({
                    "requested_name": request.admitted_name.display_name.as_str(),
                    "candidate_object_id": request.stored_object.object_id.to_string(),
                    "plaintext_sha256": hex_digest(&request.stored_object.plaintext_sha256),
                    "plaintext_size": request.stored_object.plaintext_size,
                    "encrypted_size": request.stored_object.encrypted_size,
                    "relative_path": relative_path,
                    "encryption_version": request.encryption_version,
                })
                .to_string();
                let now = chrono::Utc::now().to_rfc3339();
                transaction.execute(
                    "INSERT INTO operation_journal \
                        (journal_id, operation_type, scope_id, node_id, transaction_id, phase, \
                         payload_json, state, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, NULL, ?4, 'object_published', ?5, \
                             'applied_filesystem', ?6, ?6)",
                    rusqlite::params![
                        &journal_id,
                        OPERATION_TYPE,
                        scope_id.to_string(),
                        request.transaction_id.to_string(),
                        payload,
                        now,
                    ],
                )?;
                journal_id
            }
        };

        transaction.commit()?;
        Ok::<_, DbError>(PreparedImport {
            journal_id,
            scope_id,
            parent_node_id,
        })
    })
    .await
}

async fn publish_database_state(
    pool: &DatabasePool,
    request: ImportCommitRequest,
    prepared: PreparedImport,
) -> Result<ImportCommitReceipt> {
    pool.with_conn(move |connection| {
        let transaction =
            connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (scope_id, parent_node_id) = validate_import_target(&transaction, &request)?;
        if scope_id != prepared.scope_id || parent_node_id != prepared.parent_node_id {
            return Err(invariant(
                "import target changed after filesystem publication; transaction aborted",
            ));
        }

        let (object_id, reused_object) = persist_or_reuse_object(&transaction, &request)?;
        let (version_id, reused_version) =
            persist_or_reuse_initial_version(&transaction, object_id, &request)?;
        let display_name = resolve_display_name(
            &transaction,
            scope_id,
            parent_node_id,
            &request.admitted_name,
            request.duplicate_policy,
        )?;
        let normalized_name = display_name.to_ascii_lowercase();
        let node_id = StorageNodeId::new();
        let now = chrono::Utc::now().to_rfc3339();

        transaction.execute(
            "INSERT INTO storage_nodes \
                (node_id, scope_id, parent_node_id, node_type, display_name, normalized_name, \
                 current_version_id, system_role, state, created_at, updated_at, trashed_at) \
             VALUES (?1, ?2, ?3, 'file', ?4, ?5, ?6, NULL, 'active', ?7, ?7, NULL)",
            rusqlite::params![
                node_id.to_string(),
                scope_id.to_string(),
                parent_node_id.to_string(),
                &display_name,
                normalized_name,
                version_id.to_string(),
                &now,
            ],
        )?;

        let detected_extension = match &request.admitted_name.rule {
            FileAdmissionRule::Extension(value) => Some(*value),
            FileAdmissionRule::ExactName(_) => None,
        };
        let updated = transaction.execute(
            "UPDATE import_transactions \
             SET detected_extension = ?2, detected_mime = ?3, detected_encoding = ?4, \
                 state = 'indexing', updated_at = ?5 \
             WHERE transaction_id = ?1 AND state IN ('committing_node', 'recovering')",
            rusqlite::params![
                request.transaction_id.to_string(),
                detected_extension,
                request.detected_mime.as_deref(),
                request.detected_encoding.as_deref(),
                &now,
            ],
        )?;
        if updated != 1 {
            return Err(invariant(
                "import state changed while publishing the logical file node",
            ));
        }

        let journal_updated = transaction.execute(
            "UPDATE operation_journal \
             SET node_id = ?2, phase = 'database_committed', state = 'committed', updated_at = ?3 \
             WHERE journal_id = ?1 AND state IN ('prepared', 'applied_filesystem', 'recovery_required')",
            rusqlite::params![&prepared.journal_id, node_id.to_string(), &now],
        )?;
        if journal_updated != 1 {
            return Err(invariant(
                "import journal changed while publishing the logical file node",
            ));
        }

        transaction.commit()?;
        Ok::<_, DbError>(ImportCommitReceipt {
            journal_id: prepared.journal_id,
            scope_id,
            parent_node_id,
            node_id,
            version_id,
            object_id,
            display_name,
            reused_object,
            reused_version,
        })
    })
    .await
}

fn validate_request(request: &ImportCommitRequest) -> Result<()> {
    if request.detected_format.trim().is_empty() {
        return Err(MukeiError::Invariant(
            "detected file format must not be empty".into(),
        ));
    }
    if request.encryption_version == 0 {
        return Err(MukeiError::Invariant(
            "object encryption version must be non-zero".into(),
        ));
    }
    if request.duplicate_policy == DuplicatePolicy::ReplaceWithNewVersion {
        return Err(MukeiError::Invariant(
            "import replacement requires explicit copy-on-write versioning".into(),
        ));
    }
    Ok(())
}

fn validate_import_target(
    transaction: &rusqlite::Transaction<'_>,
    request: &ImportCommitRequest,
) -> std::result::Result<(StorageScopeId, StorageNodeId), DbError> {
    type TargetRow = (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
        String,
    );

    let row: Option<TargetRow> = transaction
        .query_row(
            "SELECT it.target_scope_id, it.target_parent_node_id, it.original_filename, it.state, \
                    s.scope_type, s.workspace_id, s.owner_chat_id, s.state, \
                    n.node_type, n.state, n.scope_id \
             FROM import_transactions it \
             JOIN storage_scopes s ON s.scope_id = it.target_scope_id \
             JOIN storage_nodes n ON n.node_id = it.target_parent_node_id \
             WHERE it.transaction_id = ?1",
            [request.transaction_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional()?;
    let row = row.ok_or_else(|| invariant("import transaction target was not found"))?;

    if !matches!(row.3.as_str(), "committing_node" | "recovering") {
        return Err(invariant(format!(
            "import is not ready for logical publication: {}",
            row.3
        )));
    }
    if row.2.trim() != request.admitted_name.display_name.as_str() {
        return Err(invariant(
            "admitted filename does not match the import transaction",
        ));
    }
    if row.7 != "active" || row.8 != "directory" || row.9 != "active" {
        return Err(invariant(
            "import target scope or parent directory is not active",
        ));
    }
    if row.0 != row.10 {
        return Err(invariant(
            "import parent directory belongs to a different storage scope",
        ));
    }

    match &request.authorization {
        ImportAuthorization::Universal => {
            if row.4 != "universal" || row.5.is_some() || row.6.is_some() {
                return Err(invariant("universal import targeted a chat workspace"));
            }
        }
        ImportAuthorization::Workspace(access) => {
            if row.4 != "workspace" {
                return Err(invariant("workspace import targeted Universal Storage"));
            }
            let workspace_id = row
                .5
                .as_deref()
                .ok_or_else(|| invariant("workspace scope is missing its workspace id"))?;
            let chat_id = row
                .6
                .as_deref()
                .ok_or_else(|| invariant("workspace scope is missing its owner chat id"))?;
            let workspace_id = parse_workspace_id(workspace_id)?;
            let chat_id = ChatId::parse(chat_id)
                .map_err(|error| invariant(format!("invalid persisted chat id: {error}")))?;
            access
                .authorize(&chat_id, workspace_id)
                .map_err(|error| invariant(error.to_string()))?;
        }
    }

    Ok((parse_scope_id(&row.0)?, parse_node_id(&row.1)?))
}

fn validate_journal_payload(
    payload: &str,
    request: &ImportCommitRequest,
) -> std::result::Result<(), DbError> {
    let payload: serde_json::Value = serde_json::from_str(payload)
        .map_err(|_| invariant("import journal payload is malformed"))?;
    let relative_path = request
        .stored_object
        .relative_path
        .to_str()
        .ok_or_else(|| invariant("object relative path is not UTF-8"))?;
    let expected_digest = hex_digest(&request.stored_object.plaintext_sha256);

    let matches = payload
        .get("requested_name")
        .and_then(serde_json::Value::as_str)
        == Some(request.admitted_name.display_name.as_str())
        && payload
            .get("plaintext_sha256")
            .and_then(serde_json::Value::as_str)
            == Some(expected_digest.as_str())
        && payload
            .get("plaintext_size")
            .and_then(serde_json::Value::as_u64)
            == Some(request.stored_object.plaintext_size)
        && payload
            .get("encrypted_size")
            .and_then(serde_json::Value::as_u64)
            == Some(request.stored_object.encrypted_size)
        && payload
            .get("relative_path")
            .and_then(serde_json::Value::as_str)
            == Some(relative_path)
        && payload
            .get("encryption_version")
            .and_then(serde_json::Value::as_u64)
            == Some(u64::from(request.encryption_version));

    if !matches {
        return Err(invariant(
            "import recovery payload does not match the published encrypted object",
        ));
    }
    Ok(())
}

fn persist_or_reuse_object(
    transaction: &rusqlite::Transaction<'_>,
    request: &ImportCommitRequest,
) -> std::result::Result<(StorageObjectId, bool), DbError> {
    let relative_path = request
        .stored_object
        .relative_path
        .to_str()
        .ok_or_else(|| invariant("object relative path is not UTF-8"))?;
    let plaintext_size = i64::try_from(request.stored_object.plaintext_size)
        .map_err(|_| invariant("plaintext size exceeds SQLite integer range"))?;
    let encrypted_size = i64::try_from(request.stored_object.encrypted_size)
        .map_err(|_| invariant("encrypted size exceeds SQLite integer range"))?;

    let existing: Option<(String, String, i64, String)> = transaction
        .query_row(
            "SELECT object_id, relative_path, encrypted_size, integrity_state \
             FROM storage_objects WHERE plaintext_sha256 = ?1 AND plaintext_size = ?2",
            rusqlite::params![
                request.stored_object.plaintext_sha256.as_slice(),
                plaintext_size,
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    if let Some((object_id, persisted_path, persisted_encrypted_size, integrity_state)) = existing {
        if integrity_state != "verified"
            || persisted_path != relative_path
            || persisted_encrypted_size != encrypted_size
        {
            return Err(invariant(
                "deduplicated object metadata is inconsistent or unverified",
            ));
        }
        return Ok((parse_object_id(&object_id)?, true));
    }

    let object_id = request.stored_object.object_id;
    let now = chrono::Utc::now().to_rfc3339();
    transaction.execute(
        "INSERT INTO storage_objects \
            (object_id, plaintext_sha256, plaintext_size, encrypted_size, relative_path, \
             detected_format, detected_mime, encryption_version, integrity_state, created_at, verified_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'verified', ?9, ?9)",
        rusqlite::params![
            object_id.to_string(),
            request.stored_object.plaintext_sha256.as_slice(),
            plaintext_size,
            encrypted_size,
            relative_path,
            request.detected_format.trim(),
            request.detected_mime.as_deref(),
            i64::from(request.encryption_version),
            &now,
        ],
    )?;
    Ok((object_id, false))
}

fn persist_or_reuse_initial_version(
    transaction: &rusqlite::Transaction<'_>,
    object_id: StorageObjectId,
    request: &ImportCommitRequest,
) -> std::result::Result<(FileVersionId, bool), DbError> {
    let existing: Option<String> = transaction
        .query_row(
            "SELECT version_id FROM file_versions \
             WHERE object_id = ?1 AND version_number = 1 \
             ORDER BY created_at ASC LIMIT 1",
            [object_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(version_id) = existing {
        return Ok((parse_version_id(&version_id)?, true));
    }

    let version_id = FileVersionId::new();
    transaction.execute(
        "INSERT INTO file_versions \
            (version_id, object_id, previous_version_id, version_number, created_by, \
             original_filename, detected_encoding, language_id, created_at) \
         VALUES (?1, ?2, NULL, 1, 'user_import', ?3, ?4, ?5, ?6)",
        rusqlite::params![
            version_id.to_string(),
            object_id.to_string(),
            request.admitted_name.display_name.as_str(),
            request.detected_encoding.as_deref(),
            request.language_id.as_deref(),
            chrono::Utc::now().to_rfc3339(),
        ],
    )?;
    Ok((version_id, false))
}
