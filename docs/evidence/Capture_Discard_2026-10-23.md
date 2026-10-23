# Retirement of explicitly discarded capture frames — 23 October 2026

Source checkpoint: `5cf9263`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

## Reproduced problem and fix

A caller retaining a sealed JournaledCaptureSession.Submission could retry it after
explicit discard and recreate the removed private journal record. The original native
receipt was absent, but the session still remembered the ticket, so normal begin-order
rejection was bypassed. The actual JNI regression failed before the fix; its original
JUnit XML is retained as capture-discard/regression-before-fix.xml. The baseline was
fc9b503 plus the new fixture; this is development regression evidence, not acceptance.

Explicit discard now retires that exact session ticket after fresh authority,
checksum and sealed-ending validation and before unlink. The existing begin-order
check rejects old frames when neither journal nor matching SQLite receipt remains.
Other pending tickets preserve their retry paths. An existing exact SQLite receipt
can still acknowledge already-published source; deleting a private recovery copy does
not delete document ink. No tombstone cache, new format or extra ticket budget is added.

Weak session keys cover interrupted sessions still held by callers without extending
session/observation lifetimes. Active-session limits are unchanged. Standalone discard
and acknowledged combined document deletion/disposition share the same validated
pre-removal hook. Failed/ambiguous native deletion never reaches it. Disposal failure
after unlink still retires old typed frames while returning an error; uncertain
filesystem durability is not represented as successful deletion. This does not promise
power-loss behavior after failed sync or prohibit deliberate future source reimport.

## Executed coverage

Development passed 130 Android tests and lint before the final fixture expansion.
Final fixtures additionally exercise active/interrupted caller sessions with standalone
discard and combined Trash/disposal followed by restore, plus an injected failure after
unlink. The failure case requires old-frame rejection while another pending ticket
remains exactly retryable. Complete frozen-source validation passed on matching clean source manifests at
5cf9263: **232 Rust tests**, **131 Android tests**, ten desktop JNI smoke groups,
format, strict Clippy, release, traceability, ARM64/ELF, Android lint, AAR payload and
manifest checks. No tests failed, were skipped or ignored. Commands, hashes, timings
and raw output are archived in capture-discard/ and latest/. E01 did not recur in this
run; this does not establish its cause or close its follow-up.

## Boundaries

No production H.I., model installation, handwriting egress or network permission was
added. Raw observations, protection checks, native source receipts and existing
SQLite/native/backup/journal formats remain unchanged. Full legacy/incomplete-record
review, physical/device behavior, optional-work scheduling and other gap groups remain
open. E01 remains an unresolved setup-timeout follow-up; no timeout/assertion was
weakened and no test was disabled.

The handover directory remains ignored/untracked and excluded from commits and pushes.
