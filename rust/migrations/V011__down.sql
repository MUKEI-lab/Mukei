DROP TABLE IF EXISTS ui_drafts;
DROP TABLE IF EXISTS ui_session_state;
UPDATE schema_metadata SET last_migration = 10 WHERE id = 1;
