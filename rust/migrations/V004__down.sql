-- V004__down.sql — Architect review GH #25.
--
-- Companion rollback for V004__branching.sql. Reverses the branch
-- graph + the active_branch_id population.

PRAGMA foreign_keys = OFF;

UPDATE conversations SET active_branch_id = NULL;
DROP INDEX IF EXISTS idx_branches_parent;
DROP INDEX IF EXISTS idx_branches_conv;
DROP TABLE IF EXISTS branches;

PRAGMA foreign_keys = ON;
