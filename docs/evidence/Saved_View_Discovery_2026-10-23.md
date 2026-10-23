# Saved writing-view discovery — 23 October 2026

Source checkpoint: `aadfcec`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full validation passed against matching clean source manifests: **204 Rust tests**,
**107 Android tests**, ten desktop JNI smoke groups, format, strict Clippy, release
build, traceability, ARM64/ELF, Android lint, AAR payload and manifest checks. No tests
failed, were ignored or skipped. Exact commands/hashes/logs are in
`continuation-2026-10-23/saved-view-discovery/` and `latest/`.

`Views` discovers existing writing-view IDs independently of last-workspace panes.
It selects master views or one authorised exercise belonging to that master, orders
by stable ID, and counts only accessible views for pagination. Existing anchor,
bookmark, exercise, entity and app checks run freshly; inaccessible identifiers are
omitted. `RestoreView` rechecks authority and returns the existing current/stale
bookmark state. Discovery creates no default view and changes no ink or saved
position. Limits are 512 returned IDs, 100,000 scanned candidates and offset at most
100,000 per request, engineering capacities without a measured performance claim.
Pagination is not a frozen snapshot; callers restart after view/access changes.

Stored view key/owner/payload mismatches and exercise/master mismatches fail closed.
Replacement uses the same read validation, preventing an existing corrupt/misbound
row from being overwritten through the normal view-save path. This requires no
schema or portable-format change and does not rewrite legacy view payloads.

Three new Rust fixtures cover reopen, visible-only pagination, current block/app
protection, master/exercise separation, invalid limits and stored identity corruption.
The actual JNI workspace-reopen fixture now discovers its saved view before restoring
the existing pane. Prior caret, copy, backup and source-retention checks also pass.
These component checks do not close full authoring/configuration acceptance.

The Q383 review confirmed existing retained assets require independent retention
authority, remain scoped to their source context, and have an explicit cancellation
operation. No cross-context asset ownership bug was established or changed.
A separate pending-draft lifecycle gap remains: recovery review uses ordinary entity
read authority, so interrupted drafts targeting a trashed/purged entity need a scoped
non-publishing review/discard path. It must preserve current/inherited locks and must
never silently replay into a retired target. This is next work, not delivered here.

`backend_gate.py` exits 2 for the eight open gap groups plus canonical/inherited
coverage only; no stale automated-evidence reason remains. All 94 requirements,
103 inherited clauses and 158 candidates remain in the ledger with no inferred PASS.
The handover folder is ignored and has no tracked files or reachable path history;
its contents were untouched. Local commits only; no push, production H.I., recogniser,
network inference or model changes.
