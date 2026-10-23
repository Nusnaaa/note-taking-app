# Owned processing work and runtime retirement — 24 October 2026

Source checkpoint: `d21583b`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

## Delivered behavior

Manager.prepare_recognition/prepare_conversion return move-only owned work that pins
an actually probed adapter version and its descriptor. Inference/conversion can run
without borrowing or locking the manager. Choosing a different default cannot silently
rebind a prepared recognition request. Existing synchronous convenience methods use the
same work path. Converter format callbacks now run inside the worker panic boundary,
not during preparation; unsupported formats remain explicit failures.

Lifecycle epochs invalidate prepared/running work when an entry is removed, replaced,
explicitly revalidated or its manager shuts down. Linked controls preserve the caller's
cancellation/deadline and any parent epochs. Cooperative adapters observe retirement;
even an adapter deliberately returning success after cancellation has its late output
rejected. Failed staged replacement probe/SQLite commit preserves the previous verified
entry/work. Explicit failed revalidation instead clears prior readiness and invalidates
its old work, matching the existing fail-closed revalidation policy.

Shared adapter ownership pins runtime/resource handles until actual work retirement.
Logical cancellation never frees a still-used resource or claims to terminate a
non-cooperative native worker. No unbounded worker queue, new JSON grant, SDK, weights,
certificate, model download or network permission is introduced. The host must reserve
worker/memory capacity and enforce current scope/source revision before publication.
Store.propose remains a pending review; accepted output and immutable source ink remain
separate. No SQLite/native/backup schema or receipt encoding changes are made.

## Executed coverage

Four new fixtures cover default-choice binding across real thread transfer, explicit
proposal acceptance without repairing deliberately wrong text, removal/revalidation/
manager-drop cancellation, concurrent removal while work is blocked, cooperative
cancellation visibility and rejection of deliberately late success, actual resource
reservation until worker retirement, exact converter code text and converter format
panic containment. Existing replacement tests now additionally require old work to
survive failed probe/commit and to be cancelled only by a successful swap.

Development passed these four fixtures plus seven engine tests, the existing 13
recognition/four converter fixtures, and strict workspace Clippy. Complete frozen-source validation passed on matching clean source manifests at
d21583b: **239 Rust tests**, **131 Android tests**, ten desktop JNI smoke groups,
format, strict Clippy, release, traceability, ARM64/ELF, Android lint, AAR payload and
manifest checks. No tests failed, were skipped or ignored. Commands, source hashes,
timings and raw output are archived in continuation-2026-10-24/processing-work/ and
latest/. E01 did not recur; this does not establish its cause or close its follow-up. These adapters/resources are synthetic application tests and make
no vendor licensing, actual recognition-quality or device-performance claim.

## Remaining integration

Bounded host/JNI recognition/proposal job transport, fresh scope cancellation,
shared scheduling, real eligible recognisers, full model-resource integration and
isolated worker termination remain open. This owned-work foundation is not a substitute
for those first-release obligations. See Recognition_Eligibility_2026-10-23.md for the
separate primary-source follow-up. E01 remains an unresolved prior setup-timeout
follow-up; no timeout/assertion was weakened or test disabled.

The handover directory remains ignored/untracked and excluded from commits and pushes.
