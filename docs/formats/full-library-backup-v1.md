# Whole-library backup v1, schema 8

This is a logical snapshot of the implemented schema, independent of native subset
v3. It covers current stored state, not unimplemented application features. The only
public backup writer emits the existing encrypted envelope described in
`native-subset-v1.md`: random data key, password-derived wrap, separate recovery-key
wrap and authenticated payload. There is no plaintext-backup writer.

Decrypted JSON has `format: "STEM-LIBRARY-1"`, `schema: 8`, `tables` and `omissions`.
Each table is an array of ordered rows. Cells are tagged `Null`, `Integer`, `Text` or
`Blob` with `value` where needed. Integer and byte fidelity must be retained. The
fixed allow-list and column widths in `core/src/backup.rs::TABLES` define all 26
schema tables. The eight SQL migrations define their column order. Restore cannot
execute SQL supplied inside a backup: table names come from the compiled allow-list
and all values are bound parameters. Unknown tables, widths or schema versions fail.

Included state covers entities and tombstones/Trash; original strokes; current and
retained block history; links/modes; exercises, heading rules, membership revisions
and reversible reassignment operations; proposals; source metadata and PDF anchors;
local image assets, groups, layouts, embeds and working views; capture retry receipts;
engine provenance/defaults; and global settings/checkpoints. Original ink inclusion
must be explicitly acknowledged when present. Every protected scope, including hidden
or trashed objects, is required and reauthenticated at output. A plan is bound to its
library ID and generation; intervening writes invalidate it.

Live source caches are always omitted. External source assets are omitted by default.
Explicit inclusion additionally requires independent retained authority and export
permission for each included asset. Revoked sources stay revoked; independently
retained authorised snapshots may survive. Model resource files are always excluded
and their machine-local paths cleared. Omissions are visible in the preview/payload.

Restore first authenticates/decrypts, bounds input, checks its fixed structure, builds
a separate in-memory schema, checks foreign keys/integrity and validates content and
relationship invariants. IDs remain stable within this new library; a new library ID
and `restore_origin` prevent prepared plans crossing into it. Source availability is
reset to Missing unless already Revoked. External grants and runtime model readiness
must be established afresh; stored Installed provenance does not mean Ready.

Optional publication requires explicit confirmation and fresh protected-scope
authentication. It writes/syncs a temporary SQLite database, then publishes a hard
link to a **new** destination and syncs the containing directory. An existing target
is never replaced. The resulting working database is plaintext SQLite under host
access controls; this does not claim at-rest encryption. Failure after publication
but before directory sync can leave the new target present with an error reported;
the original library is never overwritten. A host must handle that uncertain outcome.

Current limits: 64 MiB encrypted plaintext budget, one million total rows, schema 8
only and in-memory restore. SQLite integrity and semantic validation do not constitute
a full malformed-input security audit. Cross-version migrations, chunked/large-library
backups, Android publication/filesystem behaviour and broader crash/fuzz coverage
remain open. Password/recovery roundtrips and new-path publication are automated
Linux tests; no Android or physical power-loss result is claimed.

## Schema 9 implementation update — 22 October 2026

New writers emit schema 9 (28 allow-listed tables), including `edit_operations` and
`edit_changes`. The transaction-local `edit_operation` marker must be empty in a backup.
Restoration validates historical journal row widths, identities and content before exposing
the store. Permanent deletion also retires the entity's detailed undo payloads.
Schema-8 input upgrades with empty authoring journals; historical actions are not invented.
All other limits above remain, including the 64 MiB payload and in-memory restore.
Schemas other than 8 and 9 are rejected. This supersedes the earlier schema-8-only limit.

## Schema 15 authoring extension

Current snapshots mark schema 15. Optional per-view presets and guide backgrounds
are carried in the existing view_settings JSON payload; row widths/tables and crypto
envelope do not change. Migration 015 changes only the schema marker and leaves old
payload bytes untouched. Restore supports schemas 8–14 with authoring absent, and
uses native v9 validation for schema 15. Populated authoring under an older schema
or native version is rejected. See [native subset v9](native-subset-v9.md) for the
complete authoring field contract and view-local preset identity rules.
