-- V002__down.sql — Architect review GH #25.
--
-- Companion rollback for V002__recovery_state.sql. Drops the
-- recovery_state table. Safe to run before V001__down.sql.

PRAGMA foreign_keys = OFF;
DROP TABLE IF EXISTS recovery_state;
PRAGMA foreign_keys = ON;
