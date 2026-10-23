# Reviewed Trash, restore and permanent deletion — 23 October 2026

Source checkpoint: `1f5770e`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full validation passed against matching clean source manifests: **214 Rust tests**,
**110 Android tests**, ten desktop JNI smoke groups, format, strict Clippy, release,
traceability, ARM64/ELF, Android lint, AAR payload and manifest checks. No tests failed,
were ignored or skipped. Exact commands, hashes, timings and logs are archived in
`continuation-2026-10-23/deletion-workflows/` and `latest/`.

## Delivered behavior

The host protocol now exposes PrepareDeletion, ConfirmDeletion, ReleaseDeletion,
TrashEntries and RestoreEntity. Preparation performs dependency review with current
subtree content authority. A dependency-presence flag can report protected references,
but protected referencing entity IDs/titles are omitted. Preparation changes no
persistent content. It returns a random opaque host-local token, not a serializable
caller-forgeable core review. The action kind and library generation are backend-held.

Confirmation requires explicit true even without dependencies. It rechecks exact
library generation and fresh subtree authority in the committing operation. A true
attempt consumes the review even on failure; a false attempt leaves it available.
Released, expired, foreign-host, reopened, unknown or consumed tokens cannot delete.
Authentication replacement, relock/expiry and protection/containment/undo changes
retire reviews. Other commits invalidate their generation. PDF previews/read leases
are conservatively retired on confirmation/restore in both native and Android paths.
Ambiguous replies require inspecting current Trash state rather than assuming a commit.

Trash discovery returns only explicitly trashed, non-purged entities visible under
current app/entity/ancestor authority, ordered by stable ID. Pagination counts visible
entries only. Revision-bound restore checks the exact listed entity revision and
advances it, preserving source identity and observations. Ancestors restore first;
independently trashed children keep their own state. Permanent deletion requires a
new permanent-action review of an explicitly trashed target. It atomically retires
subtree content/assets/detailed undo while preserving honest tombstone identities.

Core retirement now handles independently trashed and previously purged descendants.
Previously purged payloads are skipped rather than making their container undeletable;
remaining content authority is preflighted before mutations. Trash, restore and purge
revision advancement checks overflow and remains transactional. There is no schema,
portable-format or source-ink serialization change.

Review capacities are 32 host-local tokens for 300 seconds, 100,000 subtree identities
and 4,096 visible dependant IDs. Trash pages return at most 512 entries with candidate
scan/offset bounds of 100,000. Oversized reviews fail rather than silently truncating
consent. The recursive identity query allocates before its size check; these engineering
bounds do not establish constant RSS or performance SLOs. Pages are not frozen snapshots.

## Executed coverage and remaining scope

Three new core fixtures cover confirmation/kind/generation binding, rollback/source
retention, current Trash permission/pagination, revision restore and mixed descendant
states. Three host fixtures cover actual ink capture/Trash/restore/purge/reopen,
redacted dependencies, stale generations, unforgeable action kind, release, cross-host
isolation, relock and protected Trash discovery. A native unit fixture covers expiry
and capacity without sleeping or deleting source. Android actual-JNI coverage adds
real Trash/restore/purge across full reopen and checks exact source ID/negative-zero
samples. The retired-capture recovery fixture now sets up real host Trash/purge instead
of synthetic persisted flags, then verifies scoped review/discard and replay rejection.
All prior tests reran; the Android validator forces rerun against the rebuilt library.

Private pending drafts remain separate recovery evidence and are never silently
retargeted/discarded by deletion. Combined deletion review showing their disposition
remains open, along with complete authoring/pane configuration and all-operation history.
No whole-backend, device interruption or final app/privacy acceptance is inferred.

All eight gap groups remain OPEN. The gate exits 2 solely for those groups and
canonical/inherited coverage; no stale automated-evidence reason remains. The ledger
retains 94 requirements, 103 inherited clauses and 158 candidates without inferred PASS.
The handover folder is ignored, untouched and absent from tracked files/reachable path
history. Local commits only; nothing pushed. No production H.I., model, recogniser,
external inference or automatic OS backup change.
