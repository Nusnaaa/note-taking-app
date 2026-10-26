# Native subset v10

Version 10 extends v9 with optional explicit pressure response in stroke style,
view authoring settings and named pen presets, and optional per-view eraser
mode/width settings. The same owner/access, independent copy, immutable original
ink, revision-safe history and protected-content contracts apply.

Readers retain versions 1–9 without inventing these fields. A pressure/eraser
extension under a pre-v10 header is unsupported. Schema 16 persists the fields
without rewriting existing payloads or capture receipts. The chosen response
changes derived width only; observed pressure remains exact, including absent
versus zero. See [pressure-response-v1.md](pressure-response-v1.md).
