-- V003__down.sql — Architect review GH #25.
--
-- Companion rollback for V003__tooling_and_saf.sql. Drops the
-- audit-log and SAF token tables. Both are append-only by design;
-- dropping them in a rollback is the explicit user-acknowledged
-- destructive action.

PRAGMA foreign_keys = OFF;
DROP TABLE IF EXISTS tool_audit_log;
DROP TABLE IF EXISTS saf_tokens;
PRAGMA foreign_keys = ON;
