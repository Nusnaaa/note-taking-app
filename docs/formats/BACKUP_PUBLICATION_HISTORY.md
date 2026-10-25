# Device-private backup publication history, version 1

This is application bookkeeping for NB-DATA-006/008, separate from the native
notebook format and its encrypted backup. It never changes the notebook's source
generation, installs resources or contacts an external file or provider. It accepts
no notebook-payload, backup-passphrase or recovery-key field. The platform SQLite
database lives at
`notebook/backup-publications/catalogue.db` under the owned app no-backup root.
It is not included in logical notebook backups. A recovered independent library
starts its own history rather than inheriting a claim that it has been backed up.

## Acknowledgements and status

A record is appended only after the existing publication protocol acknowledges:

- `LocalDurable`: verified ciphertext, no-overwrite local publication and its
  file/directory sync acknowledgement;
- `ProviderReadBack`: exact ciphertext readback after descriptor retirement, with
  descriptor-sync acknowledgement recorded separately. This is not cloud-sync or
  remote-provider durability proof.

`Publication.catalogueRecorded` and `ProviderPublication.catalogueRecorded`
acknowledge history persistence separately from file publication. A metadata-write
failure sets backup attention and returns the actual known publication outcome
with `catalogueRecorded=false`; it does not delete the file or turn a known durable
publication into an unknown file operation. Ambiguous publication itself remains
`Uncertain` and never creates a successful history record. A lost acknowledgement
after metadata commit can leave a discoverable record on restart.

`NotebookBackups.status()` returns the last acknowledged publication and the last
publication matching the current committed library identity/generation. Publishing
an older prepared artifact does not hide a newer matching record. The derived
`currentAuthoringWasPublished` is false if pending/incomplete capture evidence,
active input, queued authoring, recovery uncertainty or an unresolved authoring
failure prevents a clean claim about current work. A matching committed snapshot
can still be shown independently of that warning. Local Saved state is unchanged.

All records describe past acknowledgements. The destination may since have been
removed, changed or become inaccessible, and no status/history query probes it.
Absence of a record is not proof that no backup file exists. The external file and
its credentials remain the actual recovery path. `history(before, limit)` uses
per-library descending sequence pagination with limits 1–100; a new publication
cannot repeat an already paged record. These are defensive query bounds, not
approved device-capacity or timing SLOs.

Both queries run on the existing bounded optional backup worker. Fresh native app
authority is checked before and after private history reads. Ordinary notebook JSON
receives no file/path capability or catalogue operation. Destination strings in a
record are historical metadata, never a current URI permission or authority to
open, overwrite or delete anything.

## Stored fields and integrity

SQLite `user_version` is 1. `metadata` has exactly the purpose value
`STEM-BACKUP-CATALOGUE-1`. `publications` contains:

- non-reused autoincrement sequence and fresh record UUID (exhaustion fails rather than wrapping);
- library UUID and exact committed generation;
- local acknowledgement wall-clock time (not a trusted external timestamp);
- destination kind and bounded deliberately selected external location;
- ciphertext byte count and SHA-256;
- acknowledgement kind and descriptor-sync flag;
- original-ink inclusion, authorised source-asset count and explicit omission facts.

Named indexes cover library/sequence and library/generation/sequence. Records use
bound SQL values and strict field/type/domain checks; the expected schema/index
shape is checked before use. The format does not execute SQL from record data.
Platform SQLite is used only for this Android-owned metadata file. Rust/bundled
SQLite continues to own every notebook database and portable format.

A private directory is anchored by filesystem identity. Leaf symlinks, hardlinked
files, foreign ownership/permissions, unknown namespace entries, redirected roots,
unknown/future database versions and malformed records fail closed. Trusted app
ancestor aliases are permitted; an already anchored directory is never silently
recreated elsewhere. These are cooperative app/private-filesystem checks, not
protection against a hostile kernel or privileged concurrent filesystem writer.
Pre-existing foreign files are not initialised, repaired, migrated or deleted.

Connections are operation-local. Transactions use FULL synchronous SQLite
acknowledgements and directory sync after connection close. No raw database file
descriptor is opened alongside SQLite. Full backend close retains ownership until
actual catalogue work and its descriptors retire. Metadata errors preserve prior
records; a failed transaction cannot advance the recorded snapshot.

These are host/JNI/Robolectric-tested contracts. They do not establish device,
power-loss or real-provider durability acceptance. Optional scheduling, persisted
destination-grant consent, complete orphan salvage and broader native allocation
controls remain separate work.

For scheduled recovery-key association, an artifact reserves its first application
publication UUID before external publication. Further intentionally published
copies receive fresh record IDs; identical encrypted bytes can therefore have
multiple independent destination acknowledgements. The optional internal append
ID is canonical-UUID validated. It is not a password, portable-format parser
capability or promise that a destination remains available.
