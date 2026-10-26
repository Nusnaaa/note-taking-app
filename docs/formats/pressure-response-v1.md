# Explicit pressure appearance and eraser preferences

`StrokeStyle.pressure_response` is an optional chosen appearance response. It
never replaces, normalises or fills an observed point's pressure. Raw missing
pressure and measured zero remain distinct in capture, recovery, native copy,
backup, source fingerprints and later processing inputs.

The response contains `input_min`, `input_max`, `minimum_factor`,
`maximum_factor` and `exponent`. Every value and the input interval must be finite;
the interval is positive, factors are nonnegative/ordered with positive maximum,
and exponent is positive. Factors/exponent have defensive admission bounds of
1024, and effective source width must respect the existing stroke-width bound.
These are format/input limits, not hardware or performance SLOs.

For a present observed pressure, endpoint values saturate to the chosen factors;
interior values use the declared power response. Only derived width changes.
Absent pressure uses recorded base width and does not invent a pressure sample.
There is no automatic device-range or physical-space calibration. A missing
response keeps legacy fixed-width appearance.

## Preferences and contact ownership

A named pen preset may include a response. Applying a selected preset copies its
complete chosen colour/width/response into the current view preferences. Manual
view settings must remain consistent with a selected preset; invalid/stale updates
are rejected with the existing revision/access contract. Preference updates retain
camera/bookmarks and do not consume a document edit-Undo step.

Optional `eraser` preferences retain a `WholeStroke` or `Selection` mode and width
in millimetres. `ViewPen` snapshots both pressure appearance and eraser width in an
explicitly supplied source-unit calibration. The existing explicit stroke-erasure
operation retains original ink and its operation-level undo semantics. This change
does not infer erasure targets from a raw eraser path; gesture hit-testing, partial
segment erasure and complete nib/scene integration remain separate work.

Capture snapshots the chosen response at contact begin. Changing later settings
cannot rebind active observations. The Android style/response objects are immutable;
the native value is bounded and copied. Derived SVG uses round point caps and
connecting vector outlines with width evaluated at observed samples. Group opacity
avoids repeated alpha application at joins. Missing pressure and unsupported
orientation/tilt appearance are reported honestly. Source ink remains unchanged.

## Version boundaries

- Working SQLite schema 16 adds the feature tag only; existing stroke/view JSON,
  payload hashes and capture receipts are not rewritten. Strict existing-library
  opening accepts known schema 16 and preserves unknown/future files.
- Native subset writer version 10 carries the new fields. Readers retain versions
  1–9, and reject pressure/eraser extensions when labelled as an older version.
- Whole-library snapshot schema 16 and streaming schema 16 carry the same JSON
  fields. Aggregate readers retain known earlier snapshots; streaming readers
  accept schema 15 and 16. Schema-15 streams cannot claim new pressure metadata.
- Android private capture journal `STEMAC05` adds an optional response flag and five
  exact IEEE-754 doubles after a present style. Readers retain `STEMAC01` through
  `STEMAC04` without inventing a response. Sample bits, explicit-finger provenance,
  layer destination, interruption state and checksums retain their contracts.

No model weights, SDK, inference egress, numerical hardware claim or production
H.I. capability is introduced. Host fixtures are not actual-device calibration or
pen-performance evidence. Complete backend acceptance remains separately gated.
