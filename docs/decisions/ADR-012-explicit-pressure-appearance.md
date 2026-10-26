# ADR-012 — explicit pressure appearance with immutable source ink

Status: engineering decision under approved pen-preference/observed-ink requirements.

Store a bounded optional chosen pressure response in per-view authoring settings,
named presets and per-contact stroke style. Keep original observed pressure, null,
zero, timing and coordinate samples untouched. Do not infer a response, sensor
capability or calibration for legacy strokes. Source-unit conversion remains
explicit. This preserves the digital-ink recogniser input and independent source
fingerprint/acceptance contracts rather than introducing a raster pipeline.

Use a scalar power response with declared input endpoints/factors. Saturation and
interpolation apply only to derived appearance. Absent pressure uses the selected
base width with an honest export warning. Render configured response as vector
outlines; report remaining nib/orientation limitations. Persist eraser mode/width
alongside authoring preferences, using existing explicit reversible erasure
operations. Hit-testing/partial-segment erasure and full scene/device mapping
remain open; do not claim implementation by narrowing those requirements.

Snapshot settings at contact begin. Carry them through Android JSON/journal, JNI,
SQLite, native copy and encrypted aggregate/stream backups. Use schema 16, native
subset 10 and journal 05 feature boundaries so older metadata labels cannot hide
new appearance. Migrations change the tag only and preserve original payloads and
receipts; supported old formats remain readable. See the format contract for
limits and compatibility. No quantitative SLO or device acceptance is inferred.
