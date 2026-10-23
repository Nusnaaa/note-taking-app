# Canvas layers and independent embed visibility — 23 October 2026

Source checkpoint: `efb1437`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full exact-source validation passed with matching clean source manifests:
**201 Rust tests**, **107 Android unit/Robolectric tests**, and **ten desktop JNI
smoke groups**, with no failed/ignored/skipped tests. Format, strict workspace Clippy,
release build, traceability, ARM64 compilation/ELF, Android lint, AAR native payload
and isolated-service manifest checks passed. Commands, hashes and logs are retained
in `continuation-2026-10-23/canvas-layers/` and `latest/`.

## Delivered behavior

Canvas source blocks now have revisioned presentation-layer metadata: an implicit
visible base for unassigned/legacy blocks, ordered custom layers with stable IDs,
names, visibility and disjoint membership. Reassignment/removal changes presentation,
never source coordinates, observations, styles or ownership. Removing a layer returns
its members to the base. Invalid owner/kind/identity/membership or stale revisions
reject atomically. Fresh source/block protection applies before changes.

Each embed independently stores None (Use canvas defaults), an explicit Base/Custom
selection, or an explicitly empty selection. Duplication retains independent camera,
pin and layer preferences. Explicit overrides do not rewrite source defaults. Missing
layer references stay unresolved/invisible rather than showing all content; undo can
restore the original identity. Visibility never grants access, and existing strict
whole-source embed permissions remain enforced.

Layer-state changes and embed preferences participate in durable authoring undo.
Undo to the initial implicit base keeps an advancing revision rather than resetting
to 0, so stale edits cannot overwrite a reverted state. Reopen/redo/rollback preserve
source and metadata. Shared source/host entity revisions advance with changes. The
existing entity-touch helper now checks revision overflow instead of wrapping.

CanvasScene projects layer/z/element-kind/ordinal order and per-embed overrides.
Nested embeds currently belong to the base layer; inaccessible/broken references
produce placeholders without source/frame disclosure. Denied source blocks are
permission-checked from metadata before content loading. Pages have both item and
serialized-byte bounds, explicit next_offset and source/entity/embed revision fields.
Clients restart pagination after revision changes rather than merge stale snapshots.
An oversized single item fails instead of silently truncating. The scan cap includes
hidden items. Limits (128 custom layers, 100,000 source items, 512/page, 8 MiB output)
are implementation capacities, not measured/approved performance SLOs or RSS bounds.

Schema 14 introduces source-owned layer state and undo capture without rewriting old
source. Native subset v8 reads v1-v7 and remaps custom layer/member/override IDs,
including unresolved references, without joining copied sources to live originals.
Encrypted whole backup preserves state/history and restores schemas 8-13 with implicit
base behavior. Downgraded current and historical extensions fail closed, including
old-format snapshots whose current embed override has already been undone. Purge
retires the canvas state and detailed undo payloads. Canvas block derivative exports
respect default layer visibility with explicit omissions; native source stays retained.
Static embed/whole-scene export remains unsupported by those paths.

## Executed fixtures and limits

Eight native layer fixtures cover independent default/explicit/empty embed visibility,
missing IDs and undo identity recovery, monotonic undo/redo and rollback/reopen,
exact source retention, bad members/aliases, fresh protection and protected placeholders,
layer/z pagination, nested protected/broken references, native remapping, default-view
export filtering, encrypted state/history restoration, current/historical downgrade
rejection and byte-preserving legacy migration. A multi-megabyte fixture proves
output-byte continuation and compact protected results. Actual desktop-JNI/Robolectric
coverage changes defaults/overrides, reads both projections, and undoes the host's
preference without changing source defaults. Existing migration, style, backup,
capture and privacy suites remain included.

The initial targeted `cargo test -p stem-core --test layers --offline` run exposed a
validator helper assuming an id column on the canvas-keyed table. The helper now
selects that table's explicit canvas key and checks payload binding. All eight layer
tests then passed. Android's targeted suite passed 107 tests and lint; the complete
run above covers the committed source, including all updated legacy fixture shapes.

Still open: layer-aware capture destination/recovery, custom layer assignment for
nested embed objects, embed-local overlays/follow-object behavior, complete canvas
scene/pressure/nib/image export, authoring configurations/presets, resource scheduling,
full history/retirement, eligible recogniser integration and scalable portability.
No production layer H.I., physical tablet/rendering acceptance, model weights,
handwriting upload, remote inference, automatic OS content backup or origin push is
introduced. The handover folder stays ignored/untracked and guarded against commit/push.

The backend gate exits 2 only for substantive implementation/semantic-coverage gaps;
no stale/incomplete automated-evidence reason remains.
