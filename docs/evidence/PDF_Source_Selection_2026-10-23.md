# Trusted PDF selection and reviewed regrant — 23 October 2026

Source checkpoint: `6e4cd6d`, branch `phase-a/backend` (selection implementation
`25d6ec5` plus the timeout classification correction).

Backend acceptance remains NOT MET and production H.I. remains gated. This
increment implements source acquisition/regrant; it does not close the complete
PDF requirement or any other open backend group.

Final validation passed against matching clean source manifests: **152 Rust tests**
(no failed/ignored), **47 Android unit/Robolectric tests** (no failures/errors/skips),
and **eight desktop JNI smoke groups**. Format, strict workspace Clippy, release
build, traceability, ARM64 build/ELF, Android lint, AAR payload and isolated-service
manifest checks passed. Exact commands, hashes and logs are in
`continuation-2026-10-23/pdf-source-selection/final/` and `latest/`.

## Implemented contracts

Trusted picker selection checks read flags, a local content URI, PDF signature and
streamed SHA-256 on a bounded optional worker. Review is non-mutating: it neither
creates a source row nor persists a platform grant. The native host issues private,
bounded, expiring, single-use review capabilities and preallocates a stable source
identity. Ordinary JSON cannot manufacture a trusted provider observation.

Explicit acceptance preflights current authority, retains only READ permission when
the picker offered persistable permission, reopens and hashes provider bytes, and
requires a fresh one-use native verification. Same-revision regrant restores the
existing identity and view. Changed editions preserve revision history, move affected
anchors to their explicit review/historical states and preserve independently
retained evidence. Store transactions roll back on failure without modifying authored
blocks or original ink. Generation, scope, relock, rejection and stale-review checks
are enforced at acceptance.

Cancellation and timeouts publish failure without waiting for an uncooperative
provider; admission remains occupied until resources actually retire. Cancellation
before trusted acceptance prevents a commit. Once the native verifier accepts a
commit, the adapter waits for its actual receipt/error rather than reporting a
synthetic timeout. The OS grant and SQLite transaction are separate boundaries: a
failed database save may leave an explicitly retained READ grant, but cannot invent
a successful library receipt. The known source identity permits later inspection.

Parser disposal closes owned descriptors even if unbind/cleanup throws or its
executor rejects work. Disposal failure closes that client to prevent admission of
further isolated processes after uncertain termination.

## Evidence scope

The new fixtures exercise real desktop JNI and SQLite with synthetic Android
provider/parser/binding fixtures. They cover non-mutating review, fresh acceptance,
revoked-source restoration, changed bytes, stale generation, locked contexts,
permission loss, invalid picker inputs, single use, bounded capability admission,
transaction rollback, blocked-provider cancellation/timeout and the commit boundary.
A fault fixture blocks the adapter deadline thread and verifies provider failure is
published while retirement remains outstanding. Another forces rejected disposal
execution plus a throwing unbind and checks descriptor cleanup/client closure.

These checks establish neither real Binder isolation nor PDF rendering fidelity,
recogniser quality, actual device resource behavior or final app acceptance. No
production Activity/Compose H.I., model download, handwriting upload, automatic OS
content backup or push to origin is introduced. `handover-do-not-commit/` remains
ignored, untracked and absent from reachable branch history; local commit/push
hooks guard against its inclusion.

Managed-copy/historical-snapshot render paths, complete permission/lifecycle and
navigation workflows, and the other backend gap groups remain open.

The first full Android run at `25d6ec5` failed one of 47 methods: the provider's
own timeout won the deadline race and was wrapped as generic I/O, while the test
expected a timeout. The fix preserves content-free timeout/cancellation categories
regardless of which failure arrives first. The fixture with the blocked adapter
deadline explicitly checks the provider-originated timeout. The failed report/log
are retained under `pdf-source-selection/first-attempt/`; no failed run is counted
as passing evidence.

The final backend gate exits 2 for substantive open groups and incomplete semantic
coverage only; no stale/mismatched/incomplete automated-evidence reason remains.
