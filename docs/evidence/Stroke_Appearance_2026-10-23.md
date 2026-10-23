# Immutable chosen stroke appearance — 23 October 2026

Source checkpoint: `907d198`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full exact-source validation passed with matching clean source manifests:
**193 Rust tests**, **106 Android unit/Robolectric tests**, and **ten desktop JNI
smoke groups**, with no failed/ignored/skipped tests. Format, strict workspace
Clippy, release build, traceability, ARM64 compilation/ELF, Android lint, AAR native
payload and isolated-service manifest checks passed. Commands, source hashes and
logs are retained in `continuation-2026-10-23/stroke-appearance/` and `latest/`.

## Delivered behavior

New captures can explicitly record immutable chosen RGBA/base width in the stroke's
named source-coordinate units. The backend does not guess a physical-unit conversion
or pressure normalization. Platform conversion from mm/device units must be explicit.
Style absence on older data means unknown recorded appearance, not a claimed black
pen choice. No current view setting or sensor sample fills a missing style.

InputRouter and Android accumulator/MotionCapture/journaled sessions snapshot style
at contact begin. Later configuration affects only subsequent contacts. Completion,
active checkpoints, lifecycle interruption, restart review and exact-ticket retry
preserve the original style and samples. Rebound headers/receipts fail; invalid
width/channel values cannot partially replace active configuration. Kotlin style
channels are immutable scalars rather than caller-owned mutable arrays.

Schema 13 adds optional source metadata without rewriting old payloads, hashes or
capture receipts. Native subset v7 preserves style and reads v1-v6; older tags cannot
hide styled source. Encrypted schema-13 backups preserve style and restore schemas
8-12 with the corresponding feature limits. Android STEMAC02 journals store exact
RGBA bytes/width bits and retain a strict STEMAC01 reader with no invented style.
A frozen independently encoded v1 fixture recovers exact negative zero/measured zero
and publishes only explicit recovery metadata under v2. Original source observations
remain independent of corrections, recognition, presentation and expiring undo.

Original-ink SVG now uses recorded colour, alpha and source-space base width, round
paths and single-sample dots. Bounds include stroke radius so wide/dot marks are not
clipped. Export warnings explicitly identify legacy fallback, unreconstructed
pressure/tilt-dependent appearance and eraser observations shown as source paths.
Raw/native data still contains the exact original channels and tool; no destructive
eraser operation, pressure ribbon or handwriting correction is guessed by export.

## Executed scope and limits

Five native fixtures cover begin-bound style, invalid configuration, immutable headers,
exact retry/rebound rejection, reopen/correction/native copy, legacy format tags,
encrypted style backup/schema-12 downgrade rejection, byte-preserving schema-12
migration with the original receipt, and SVG RGBA/width/dots/fidelity warnings.
Existing export fixtures remain included. Android fixtures cover begin-bound style
and old wire omission, a frozen v1 journal, exact v2 style recovery/rebound rejection,
and actual JNI capture plus retry after a mid-contact configuration change. The JNI
fixture also checks full-precision style width and distinct styles on later contacts.

The initial targeted Android run (`:backend:testDebugUnitTest :backend:lintDebug
--offline`) completed 106 tests with one fixture failure: a plain JVM accumulator
test called Android's stubbed JSONObject. That parser call was removed; actual JSON
fields remain covered by the real JNI/Robolectric fixture. The final complete run
above validates the corrected committed source without skipped tests. Native targeted
style/export tests and strict Clippy passed before the full run.

Still open: complete physical-space mapping, pen presets/backgrounds, nib/pressure
rendering, whole-scene/transform fidelity, remaining pane/authoring workflows,
resource scheduling, full history/retirement, model lifecycle, scalable portability
and complete semantic acceptance. These synthetic tests establish no physical tablet
appearance/latency or power-loss guarantee. No production H.I., recogniser weights,
handwriting upload, remote inference, automatic OS content backup or origin push is
introduced. The handover folder remains ignored/untracked and guarded against
commit/push.

The backend gate exits 2 only for substantive open implementation groups and
incomplete canonical/inherited coverage, with no stale/incomplete automated-evidence
reason.
