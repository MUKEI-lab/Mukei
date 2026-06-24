-- V001__down.sql — Architect review GH #25.
--
-- Companion rollback for V001__schema.sql. Drops every table created
-- by V001 in reverse-FK order so a SQLCipher-encrypted database can be
-- safely rewound to a pre-V001 state during a bad-migration recovery
-- (PRD REQ-SEC-19 + BS §14).
--
-- WARNING: rollback DESTROYS user data. The bridge crate's recovery
-- path prompts the user with a destructive-action confirmation BEFORE
-- invoking this file. It is NEVER run automatically.

PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS chunks;
DROP TABLE IF EXISTS messages;
DROP TABLE IF EXISTS conversations;
DROP TABLE IF EXISTS migrations_applied;

PRAGMA foreign_keys = ON;
