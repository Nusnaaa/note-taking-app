# Native host backup lifecycle — 24 October 2026

Source checkpoint: `9a1cb79`, branch `phase-a/backend`.
Backend acceptance remains NOT MET; production H.I. remains gated.

## Delivered behavior

The native host now admits one instance-bound backup review/job. Review metadata is
bound to current access, source generation and the shared capture/access epoch; its
scope/input estimate is bounded by the host query-byte allowance. A retained old or
foreign review cannot release or run a new reservation. Cancelled idle metadata is
reclaimed on an optional review caller or close, without pinning library ownership
through an idle caller-held review. No background worker is required merely to open
or author a library.

The writer only reviews, opens the private core cursor, pulls bounded frames, validates
and retires its state. Password derivation, encryption and owned sink write/flush/drop
execute on the caller's optional worker. Fresh trusted scoped proof and explicit
confirmation precede output. Capture, access changes/expiry, host close, caller control
and source edits reject late completion; new authentication/pen-up cannot revive old
work. Existing parent JobControl epochs remain linked. No JSON/model input can fabricate
these capabilities, a sink, protected proof or runtime readiness.

The sink is consumed, never returned. Only after its actual retirement, current source/
authority validation and cursor cleanup may BackupReceipt expose a Zeroizing recovery
key. Callback and destructor panic boundaries are separate so a write panic does not
unwind through the owned sink. Reentrant callback close rejects self-join. Close retains
exclusive library ownership through actual worker/sink/cursor retirement, including a
sink destructor blocked after the encrypted stream is already complete.

One fixed lifecycle-only cleanup slot prevents ordinary query saturation from stranding
the actor cursor. It runs after authoring and before ordinary reads, may add one to the
query count, and carries no content bytes. Ordinary data-query bounds/authoring FIFO
are preserved. Guards release metadata/worker reservations only after owned retirement.
Authoring and recognition remain independent of ordinary optional backup failures.

A receipt acknowledges encrypted committed-SQLite-stream completion and owned sink
retirement, not OS file sync/publication or coverage of private capture recovery stores.
JNI/Android must bind trusted quota/sync/publication and pending-draft review before an
app-level backup success. Working SQLite/private scratch privacy and legacy formats,
original stroke payloads/checksums, explicit retained-source authority and fresh-auth
no-overwrite restore publication remain unchanged.

## Executed coverage

Eleven new native integration fixtures exercise original ink/content and reopened restore,
protected scope/fresh proof/confirmation, foreign/closed review rejection, actual export/
restore above the legacy 64 MiB limit, optional metadata budget failure, writing while a
sink blocks, capture/source cancellation, admission limits, verifier/write/destructor
panics, reentrant destructor close, saturated-query cleanup and exclusive ownership
through both blocked write and post-encryption destructor retirement. These are synthetic
application tests; no private handwriting, generated archive, keys, SDK or model is
committed. No device SLO or hard non-cooperative termination claim is inferred.

Development passed the eleven fixtures, existing host/recognition/pull regressions,
a final capture-cancellation check and strict workspace Clippy. Final frozen-source validation passed at 9a1cb79: **294 Rust tests**, **135 Android
tests**, ten desktop JNI smoke groups, format, strict Clippy, release, traceability,
ARM64/ELF, Android lint, AAR and merged-manifest policy. Both runners finished with
clean matching source manifests. No tests failed, were skipped or ignored in these
final runs. Commands, hashes, timings and raw output are archived under host-backup/
and latest/. Android results establish build/boundary regressions, not JNI backup
transport, which remains unimplemented. E01 did not recur; its cause is unestablished.

## Remaining work

JNI/Android trusted transport, protected review/bootstrap and recovery-draft workflows,
platform sink quota/sync/atomic publication, full resource scheduling and device/privacy
acceptance remain open. Initial metadata/proof preparation still scales with the library
and needs fully scheduled/bounded integration. Semantic restore validation still builds
a whole graph; staging disk quota/crash cleanup and all other backend gap groups/semantic
coverage remain open. No new SDK/model, network permission or production H.I. is added.
E01 retains its separate historical follow-up.

The handover folder remains ignored, untracked and absent from local path history, with
local commit/push guards intact. No push was performed.
