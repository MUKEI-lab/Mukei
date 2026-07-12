DROP INDEX IF EXISTS idx_storage_reservations_expiry;
DROP TABLE IF EXISTS storage_reservations;
DROP INDEX IF EXISTS idx_download_jobs_status_updated;
DROP TABLE IF EXISTS download_jobs;
-- SQLite cannot safely DROP the added document_tombstone columns in-place.
