# Explicit Android finger capture — 23 October 2026

Source checkpoint: `78436b1`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

## Delivered behavior

The Android backend accepts finger ink only when explicitly enabled for a capture
session. The host can supply the persisted authorised ViewPen.finger_ink snapshot;
settings are not an access grant. Changing this runtime setting affects new contacts,
while an admitted active finger contact retains its tool identity until it ends.
Default finger contact produces no source ink. New stylus/eraser contacts cancel
uncommitted tentative finger contacts; new fingers during stylus contact are ignored.
Previously committed finger ink is unchanged. Android tool classification is observed;
physical palm rejection, navigation and completed-app gesture acceptance remain open.

CapturedStroke gains an optional explicitFinger flag, default false, mutually exclusive
with eraser. Encoding uses the existing native ExplicitFinger tool. Chosen style,
original coordinates, observed optional channels, relative event timestamps, completion
and interruption provenance remain independent. No synthetic pen-up or raster data is
introduced. Contact settings never rewrite existing source strokes or their receipts.

Private active journal STEMAC04 preserves the preceding layout and changes the eraser
boolean byte to a strict tool code (0 Pen, 1 Eraser, 2 ExplicitFinger). Readers accept
STEMAC01/02/03 with their original strict boolean semantics and absent older style/layer
metadata. Recovery rewrites metadata using v4 without inventing finger classification.
Tool identity is immutable across checkpoint extensions and survives recovered-prefix
review, approval and replay. SQLite/native document versions are unchanged.

## Executed coverage

Four additional Android fixtures cover opt-in and mid-contact setting changes,
stylus preemption, default suppression, cancellation, immutable journal tool identity,
recovery/replay, v3 layer compatibility and actual JNI source persistence across reopen.
Existing v1/v2 golden fixtures remain. Final source also asserts finger suppression
while a stylus is active and cancellation of opted-in finger contact.

The development suite passed 123 Android tests and lint before the final assertion
additions. Complete validation then passed on matching clean source manifests at
78436b1: **230 Rust tests**, **123 Android tests**, ten desktop JNI smoke groups,
format, strict Clippy, release, traceability, ARM64/ELF, Android lint, AAR payload and
manifest checks. No tests failed, were skipped or ignored. Commands, hashes, timings
and raw output are archived in continuation-2026-10-23/finger-capture/ and latest/.
The prior E01 fixture timeout did not recur; its cause is not established by this run.

## Boundaries

No production Activity/Compose, handwriting upload, model/SDK/weights installation or
network permission was added. Physical coordinate calibration, pressure/eraser behavior,
optional-work scheduling, full rendering and other backend gap groups remain open.
The prior E01 PDF fixture setup timeout remains an unresolved evidence follow-up;
no timeout/assertion was weakened or test disabled here.

The handover directory remains ignored and untracked; reachable Git path history was
checked and local pre-commit/pre-push guards verified. It was neither staged nor pushed.
