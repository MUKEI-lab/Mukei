async fn load_authorized_committed_receipt(
    pool: &DatabasePool,
    request: &ImportCommitRequest,
) -> Result<Option<ImportCommitReceipt>> {
    let request_for_db = request.clone();
    let authorization_record: Option<(String, String, Option<String>, Option<String>)> = pool
        .with_conn(move |connection| {
            connection
                .query_row(
                    "SELECT j.payload_json, s.scope_type, s.workspace_id, s.owner_chat_id \
                     FROM operation_journal j \
                     JOIN storage_scopes s ON s.scope_id = j.scope_id \
                     WHERE j.transaction_id = ?1 AND j.operation_type = ?2 \
                       AND j.state = 'committed' \
                     ORDER BY j.created_at DESC LIMIT 1",
                    rusqlite::params![request_for_db.transaction_id.to_string(), OPERATION_TYPE],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()
                .map_err(DbError::from)
        })
        .await?;

    let Some((payload, scope_type, workspace_id, owner_chat_id)) = authorization_record else {
        return Ok(None);
    };

    match &request.authorization {
        ImportAuthorization::Universal => {
            if scope_type != "universal" || workspace_id.is_some() || owner_chat_id.is_some() {
                return Err(MukeiError::Invariant(
                    "committed import retry is not authorized for this storage scope".into(),
                ));
            }
        }
        ImportAuthorization::Workspace(access) => {
            if scope_type != "workspace" {
                return Err(MukeiError::Invariant(
                    "committed workspace retry targeted Universal Storage".into(),
                ));
            }
            let workspace_id = workspace_id.as_deref().ok_or_else(|| {
                MukeiError::Invariant("workspace retry is missing its workspace id".into())
            })?;
            let owner_chat_id = owner_chat_id.as_deref().ok_or_else(|| {
                MukeiError::Invariant("workspace retry is missing its owner chat id".into())
            })?;
            let workspace_id = Uuid::parse_str(workspace_id)
                .map(WorkspaceId)
                .map_err(|_| MukeiError::Invariant("persisted workspace id is invalid".into()))?;
            let chat_id = ChatId::parse(owner_chat_id).map_err(|error| {
                MukeiError::Invariant(format!("invalid persisted chat id: {error}"))
            })?;
            access
                .authorize(&chat_id, workspace_id)
                .map_err(|error| MukeiError::Invariant(error.to_string()))?;
        }
    }

    validate_journal_payload(&payload, request).map_err(MukeiError::from)?;
    load_committed_receipt(pool, request.transaction_id).await
}
