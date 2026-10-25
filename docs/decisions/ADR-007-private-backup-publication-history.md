# ADR 007 — device-private backup publication history

25 October 2026. Engineering continuation of the approved backend-first strategy.

The portable Rust library remains the notebook/content authority. Android backup
publication has device-specific file/provider acknowledgements that are not part
of the portable snapshot. Store their bounded metadata records in a separate
platform SQLite database under the already owned no-backup root. Do not write the
record into the notebook and thereby make its own source generation stale.

History records are past acknowledgements, not current external-file availability
or remote cloud durability. Preserve inclusion/exclusion facts. Record matching
committed state separately from unfinished capture/authoring warnings and local
Saved receipts. A catalogue failure cannot conceal an already published file or
silently manufacture a backed-up acknowledgement.

Use the existing optional backup worker and full-close ownership boundary. Scope
history reads with fresh app checks; bind record insertion to the actual prepared
snapshot and successful publication path. Backup passphrases, recovery keys, notebook payloads and live source
URI grants are not copied into the catalogue. Do not contact destinations during status
queries or acquire persistent URI permission automatically.

Use only the metadata file with Android SQLite; never open a Rust-owned notebook
file through the platform SQLite coordinator. Preflight private regular files and
single-link identity before opening; keep unknown files and future schemas intact.
Anchor actual directory identity while allowing trusted app ancestor aliases.
Sync directory descriptors only after SQLite closes its own metadata connection.

No production H.I., new model/runtime strategy, commercial scope or cloud-sync
feature is introduced. See [the format contract](../formats/BACKUP_PUBLICATION_HISTORY.md) for the versioned fields
and acknowledgement semantics. Backend/whole-requirement acceptance remains open.
