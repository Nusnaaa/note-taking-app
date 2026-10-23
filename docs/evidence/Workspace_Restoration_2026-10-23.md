# Private workspace restoration — 23 October 2026

Source checkpoint: `018c696`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full exact-source validation passed with matching clean source manifests:
**188 Rust tests**, **102 Android unit/Robolectric tests**, and **ten desktop JNI
smoke groups**, with no failed/ignored/skipped tests. Format, strict workspace
Clippy, release build, traceability, ARM64 compilation/ELF, Android lint, AAR native
payload and isolated-service manifest checks passed. Commands, hashes and logs are
retained in `continuation-2026-10-23/workspace-restoration/` and `latest/`.

## Delivered behavior

The library-local workspace records stable opaque pane slots, order, integer relative
weights, horizontal/vertical arrangement, active slot or library route, selected tool
and existing saved-view/PDF references. Saved views retain note/canvas/exercise identity,
camera, pen settings and revision-bound bookmarks. Ordinary entity reads resolve
notebook ancestry. The default workspace is empty at revision 0. Optimistic updates
commit atomically; structural/access/revision failures retain the previous workspace.
Eight panes and integer weights 1..1,000,000 are implementation capacities, not SLOs.
Tool metadata records intent without enabling a tool or starting recognition.

Restoration checks app access first, then current view/source/block/exercise authority.
Locked or unavailable panes expose only opaque slot/weight placeholders, without
private target IDs, tools, titles or locators. Other authorised panes remain available.
Keep retains an existing slot's exact private target/tool for layout adjustment; it
cannot reveal or rebind that target and fails for a new/forged slot. Explicit targets
need fresh access. Closing a locked pane changes navigation metadata, never authored
content. SQL identity guards prevent slot reuse of persisted content IDs and later
view/stroke ID reuse, so placeholders cannot echo private target identifiers.

PDF panes restore authorised relationship/view metadata with
requires_platform_validation=true, even when stored availability is Available.
No provider/parser opens, source reads, read leases, OS grants or runtime readiness
follow from workspace restoration. The trusted selection/render pipeline remains
mandatory. Layout changes never transform observed ink; workspace receipts acknowledge
only workspace metadata, not active capture or a document-save boundary.

Schema 12 adds relational workspace state. Permanent view/source deletion atomically
retires pane targets/tools and advances workspace revision while retaining stable
slot order/weights/focus as unavailable placeholders. Trash/restore retains existing
references without reattachment. Encrypted whole backups preserve workspace metadata;
schemas 8-11 default to an empty workspace and reject new workspace tables under old
tags. Restored Available/Missing PDF sources are Missing; existing Revoked sources
stay Revoked. Native v6 copies keep selected saved views but never activate or overwrite
a destination workspace; the omission report states that activation is library-local.

## Executed fixtures and limits

Seven workspace fixtures cover independent master/exercise views and other notes,
reopen with PDF position/camera/tools/focus, original samples, fresh protection and
redaction, Keep, app lock, layout bounds/CAS, rollback, slot identity collisions,
clear, Trash/restore, purge rollback/reference retirement, encrypted restore, malformed
order/downgraded schemas, default schema-11 migration and unchanged view payloads.
Native copy is checked against both empty and already-active destination workspaces.
A host fixture and actual desktop-JNI/Robolectric fixture fully close/reopen and discover
the last view/tool; the host also checks redaction after relock. Existing position,
interrupted-capture, legacy backup and migration suites remain included.

The targeted development command `cargo test -p stem-core --test workspace --test
positions --test interrupted_capture --test host --offline` initially found a fixture
expecting Revoked instead of the existing Missing restore policy. The fixture was
corrected, preserving implementation behavior. The workspace-only rerun passed all
seven tests. The Android pre-check passed 102 tests and lint before final slot guards;
the complete run above validates all final committed inputs. Legacy synthetic fixtures
now remove workspace tables/triggers when constructing genuine older schemas, while
bookmark/interruption downgrade fixtures still test those specific feature tags.

Remaining: complete pen/background/preset configuration, pane workflows, optional-work
resource scheduling, operation history/retirement, rich-object flow/export, model
lifecycle, scalable portability and complete semantic acceptance. Tablet window/gesture,
physical pen, process/storage interruption and runtime resource acceptance are not
established by these synthetic backend fixtures. No production H.I., weights,
handwriting upload, remote inference, automatic OS content backup or origin push is
introduced. The handover folder remains ignored/untracked and guarded against local
commit/push.

The backend gate exits 2 only for substantive open implementation groups and incomplete
canonical/inherited coverage, with no stale/incomplete automated-evidence reason.
