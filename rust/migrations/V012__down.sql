DROP INDEX IF EXISTS idx_document_ingestion_state_updated;
DROP TABLE IF EXISTS document_ingestion_jobs;
-- SQLite 3.35+ supports DROP COLUMN. Production migrations are forward-only;
-- this down file exists for local development rollback only.
ALTER TABLE saf_tokens DROP COLUMN os_permission_state;
