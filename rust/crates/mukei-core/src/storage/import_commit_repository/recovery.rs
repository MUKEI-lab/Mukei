async fn load_authorized_committed_receipt(
    pool: &DatabasePool,
    request: &ImportCommitRequest,
) -> Result<Option<ImportCommitReceipt>> {
    let request = request.clone();
    pool.with_conn(move |connection| {
        type ReceiptRow = (
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        );

        let row: Option<ReceiptRow> = connection
            .query_row(
                "SELECT j.journal_id, j.scope_id, n.parent_node_id, n.node_id, \
                        n.current_version_id, fv.object_id, n.display_name, j.payload_json, \
                        s.scope_type, s.workspace_id, s.owner_chat_id \
                 FROM operation_journal j \
                 JOIN storage_nodes n ON n.node_id = j.node_id \
                 JOIN file_versions fv ON fv.version_id = n.current_version_id \
                 JOIN storage_scopes s ON s.scope_id = j.scope_id \
                 WHERE j.transaction_id = ?1 AND j.operation_type = ?2 \
                   AND j.state = 'committed' \
                 ORDER BY j.created_at DESC LIMIT 1",
                rusqlite::params![request.transaction_id.to_string(), OPERATION_TYPE],
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

        let Some(row) = row else {
            return Ok(None);
        };

        match &request.authorization {
            ImportAuthorization::Universal => {
                if row.8 != "universal" || row.9.is_some() || row.10.is_some() {
                    return Err(invariant(
                        "committed import retry is not authorized for this storage scope",
                    ));
                }
            }
            ImportAuthorization::Workspace(access) => {
                if row.8 != "workspace" {
                    return Err(invariant(
                        "committed workspace retry targeted Universal Storage",
                    ));
                }
                let workspace_id = row
                    .9
                    .as_deref()
                    .ok_or_else(|| invariant("workspace retry is missing its workspace id"))?;
                let chat_id = row
                    .10
                    .as_deref()
                    .ok_or_else(|| invariant("workspace retry is missing its owner chat id"))?;
                let workspace_id = parse_workspace_id(workspace_id)?;
                let chat_id = ChatId::parse(chat_id)
                    .map_err(|error| invariant(format!("invalid persisted chat id: {error}")))?;
                access
                    .authorize(&chat_id, workspace_id)
                    .map_err(|error| invariant(error.to_string()))?;
            }
        }

        validate_journal_payload(&row.7, &request)?;
        Ok(Some(ImportCommitReceipt {
            journal_id: row.0,
            scope_id: parse_scope_id(&row.1)?,
            parent_node_id: parse_node_id(&row.2)?,
            node_id: parse_node_id(&row.3)?,
            version_id: parse_version_id(&row.4)?,
            object_id: parse_object_id(&row.5)?,
            display_name: row.6,
            reused_object: true,
            reused_version: true,
        }))
    })
    .await
}
