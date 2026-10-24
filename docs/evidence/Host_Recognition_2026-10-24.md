# Native recognition workflow — 24 October 2026

Source checkpoint: `ef42972`, branch `phase-a/backend`.
Backend acceptance remains NOT MET; production H.I. remains gated.

## Delivered behavior

The trusted Rust host now authorises immutable digital-ink source snapshots and
reserves bounded count/encoded-byte capacity through preparation, inference and actual
retirement. Verified owned work runs off the writer. Current source epochs, exact access
expiry, caller cancellation/deadline and manager runtime retirement prevent stale
publication. Cleanup of cancelled idle runtimes executes on a lazy optional retirement
lane, with running resources and exclusive library ownership retained until actual
retirement. Worker reentrant shutdown is rejected without self-join. Optional worker
startup failure leaves authoritative ink authoring available.

Completion publication is a native typed operation, not a JSON output/readiness grant.
It creates a pending review using a stable ticket; retry acknowledges the same current
correction/state without overwriting it. Protected paged summaries discover reviews
after restart without a model. Explicit correction, rejection and acceptance cross the
existing host/JNI JSON boundary. Original observations remain recoverable.

Disjoint selected regions of one block can now be accepted together atomically. Each
block advances once; each document receives its own Undo entry. Duplicate regions and
whole-block/region overlap fail before mutation. A genuine pre-fix development failure
is retained in host-recognition/region-batch-before-fix.log; it is development evidence,
not an acceptance run or a result attributed to the final source checkpoint.

The source caps are engineering admission bounds, not SDK memory/latency measurements.
No model, SDK, certificate, weights, network permission, H.I., logical schema version or
receipt-format change is introduced. Synthetic adapters test orchestration only.

## Validation

Development passed the native host workflow fixtures, related processing/recognition/
engine/editing suites, strict workspace Clippy, Android tests and lint. Full frozen-source
validation passed at ef42972: **258 Rust tests**, **132 Android tests**, ten desktop JNI
smoke groups, format, strict Clippy, release, traceability, ARM64/ELF, Android lint, AAR
payload and merged-manifest policy. There were no failed, skipped or ignored tests. Both
runners finished with clean matching source manifests. Commands, hashes, timings and
raw output are archived in continuation-2026-10-24/host-recognition/ and latest/. E01
did not recur; its cause remains unestablished.

## Remaining work

JNI/Android trusted recogniser-provider registration/execution, shared capture/model
scheduling, an actually eligible approved runtime and isolated hard termination remain
open. Other substantive backend gap groups and semantic requirement coverage also remain
open. See backend-gaps.json and HOST_INTEGRATION.md. E01 remains an open follow-up; no
passing rerun establishes the cause of the retained historical fixture timeout.

The handover folder remains ignored, untracked and absent from local path history.
Existing local pre-commit/pre-push guards are intact. No push was performed.
