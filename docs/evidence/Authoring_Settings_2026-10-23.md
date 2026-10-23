# Persisted pen presets, calibration and guide backgrounds — 23 October 2026

Source checkpoint: `2f4d686`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full validation passed against matching clean source manifests: **230 Rust tests**,
**119 Android tests**, ten desktop JNI smoke groups, format, strict Clippy, release,
traceability, ARM64/ELF, Android lint, AAR payload and manifest checks. No tests failed,
were ignored or skipped in this run. Commands, hashes, timings and logs are archived
in `continuation-2026-10-23/authoring-settings/` and `latest/`.

## Delivered behavior

ViewSettings has optional authoring configuration: ordered view-local named RGBA/width
presets, an explicit selected preset and an optional Blank/Ruled/Dotted/Graph background.
ConfigureView and ApplyViewPreset check revision, current view authority and complete
shape/selection validity; selected appearance matches current basic pen fields. Missing
IDs, invalid values and stale revisions reject without clamping or choosing a fallback.
Removing the marker retains the current custom appearance. Preferences preserve camera,
anchor/finger settings and exact bookmark data, including NeedsReview for stale carets.
They do not consume document edit Undo or rewrite existing source observations.

ViewPen returns a current authorised snapshot with declared coordinate space and base
stroke width computed from pen_width_mm times an explicit host-supplied units_per_mm.
Nonfinite/invalid/out-of-range products reject. This provides deterministic calibration
math without inventing device DPI or physical accuracy. Existing capture setStyle
semantics snapshot it at contact begin; later preference changes cannot rebind active
headers or transform measured points. Pressure response and eraser configuration remain
open, and no device-precision acceptance is inferred.

ViewGuides generates a bounded standalone SVG component in requested source bounds,
with escaped coordinate labels and explicit view revision/configured/omitted metadata.
It reads no source strokes. include_in_export=false suppresses guides and paper fill
when for_export is requested. Missing legacy configuration produces no invented guide;
Blank can retain a chosen paper fill. Pattern output is bounded and excessive logical
guide density rejects. Existing full scene/PDF exporters do not automatically composite
this component; that integration remains open.

Schema 15 marks these optional view JSON fields without adding tables or rewriting
legacy rows. Native subset v9 reads v1-v8, preserves scoped preset IDs while global
view/entity IDs remap, and rejects populated authoring under older tags. Encrypted
schema-15 whole backup restores schemas 8-14 without inventing authoring. Row widths,
crypto envelope, source-ink/receipt and capture journal formats stay unchanged. The new
format extension is documented in docs/formats/native-subset-v9.md. Internal optional
configuration uses heap indirection to keep host command layout bounded; JSON is unchanged.

## Executed coverage and remaining scope

Six new native fixtures cover selection/calibration/reopen/source invariance, stale
bookmark/camera preservation without consuming source Undo, current protected-anchor
checks, all guide kinds/XML escaping/export omission/density limits, invalid/dangling
selection and rollback, scoped native copy, schema-15 encrypted restore and older-tag
rejection, and byte-preserving schema-14 migration. A schema-14 snapshot with authoring
removed restores its legacy basic width without fabricating presets. Existing migration,
workspace, layer, style, backup and positional fixtures reran with current schema/version
expectations and their original compatibility assertions retained.

Actual JNI/MotionEvent coverage changes presets during an active contact, explicitly
supplies calibration/style for future contacts, verifies 2.4/4.8 source-unit widths
and unchanged measured coordinates, checks export omission and restores the selected
preset/camera after full backend reopen. All final Android tests were forced to rerun
against the rebuilt library. This establishes component behavior, not full authoring
or field-level requirement closure.

Remaining work includes complete gesture/session-setting and pane integration,
pressure/eraser configuration, verified physical-space/device mapping, full scene
composition/export, operation-history/resource scheduling and the other registered
backend gaps. Prior E01 setup-timeout evidence remains retained with cause unestablished;
it did not recur in this full run. Owner-private pen/model tests remain deferred.

All eight gap groups remain OPEN. The gate exits 2 solely for those groups and
canonical/inherited coverage, without stale automated-evidence reasons. The ledger
retains 94 requirements, 103 inherited clauses and 158 candidates without inferred PASS.
The handover folder is ignored, untouched and absent from tracked files/reachable path
history. Local commits only; nothing pushed. No production H.I., model, recogniser,
external inference or automatic OS backup change.
