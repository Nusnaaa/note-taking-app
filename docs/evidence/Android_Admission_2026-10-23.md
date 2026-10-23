# Android writer admission and status — 23 October 2026

Source checkpoint: `3f499f9`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full exact-source validation passed with matching clean source manifests:
**172 Rust tests**, **100 Android unit/Robolectric tests**, and **ten desktop JNI
smoke groups**, with no failed/ignored/skipped tests. Format, strict workspace
Clippy, release build, traceability, ARM64 compilation/ELF, Android lint, AAR native
payload and isolated-service manifest checks passed. Commands, source hashes and
logs are retained in `continuation-2026-10-23/android-admission/` and `latest/`.

## Delivered behavior

The Android single writer now reserves separate authoring/query counts and logical
input-byte budgets, including executing actions. Capture, edits, authentication and
relock preserve FIFO and run before queued optional reads. Unknown/new commands
retain authoring semantics until deliberately classified. Read classification follows
the native host, including PDF read leases. A running action is never preempted;
continuous authoring may defer queued reads. Rejected work remains the caller's
responsibility. Cancelling a returned observation cannot retract admitted ink.
Shutdown closes admission and drains both lanes without interrupting transactions.

Defaults are 64 authoring actions/32 MiB and 16 queries/8 MiB. These are bounded
implementation capacities, not approved performance SLOs. Strings charge a conservative
four bytes per UTF-16 code unit plus envelope space; typed capture charges metadata,
retained strings and 128 bytes per observed point. Typed JSON encoding remains on
the writer. Queued request closures retain the input string rather than its parsed
JSON tree; recovery decisions capture ticket/entity/checksum metadata rather than
the full review stroke. Authentication scope strings obey the existing JNI character
limit before admission.

Immediate writer status and pull-based coalesced subscriptions expose only sequence,
counts, byte charges, sticky authoring failure and shutdown/termination flags. Slow
observers read the latest snapshot with no event backlog or writer status callbacks.
Native authoring rejection also marks attention. Query rejection does not claim a
failed edit. Admission counters are never recovery or document-save receipts;
writer termination does not acknowledge subsequent close-time sealing/native or
provider/parser retirement. Use per-ticket receipts, capture recovery state and
closeFullyAsync for those respective boundaries.

## Executed fixture scope and limitations

Six deterministic writer tests gate a running action to check reserved counts and
bytes, no cross-lane borrowing, executing-input accounting, FIFO edits over queued
reads, capacity recovery, cancelled observers, draining shutdown, sticky failure and
coalesced subscriptions. Four actual desktop-JNI/Robolectric fixtures establish:

- A full 16-query lane still admits synthetic MotionEvent checkpoints/completion;
  queued reads execute afterwards and observe the committed stroke.
- A full 64-action authoring lane rejects a checkpoint while retaining the caller's
  immutable observed frame, which retries with the same identity once capacity returns.
- One oversized Search input consumes byte capacity before count; a second is rejected
  while capture remains admitted. Native Search validation then rejects the oversized
  query, without misreporting a failed authoring action.
- Capture, relock and later capture keep FIFO. A read queued earlier checks current
  authority after relock; denied capture remains recoverable and replays on fresh
  scoped authentication without duplication.

The targeted pre-check passed 99 Android tests and lint before the final rejection/
retry fixture and queued-reference refinements. The complete run above covers the
final committed source and all 100 tests. Existing capture, ownership, PDF/provider,
parser, retention and scoped recovery fixtures remain included.

Logical input budgets do not bound whole-process RSS, caller-owned observations,
native allocations, returned content, trusted callback closures, replay work or total
storage. CompletableFuture dependent callbacks may run on the completing thread;
callers must dispatch expensive callbacks elsewhere. Automated tablet resource,
Binder/process, physical pen latency and power-loss acceptance remain unproven.
Full optional-work/resource scheduling and other rich-object, lifecycle, portability
and semantic acceptance gaps remain in the register. No early H.I., recogniser
weights, uploads, remote inference, automatic OS content backup or origin push is
introduced. The handover folder remains ignored and untracked, with commit/push
guards installed.

The backend gate exits 2 only for substantive open implementation groups and
incomplete canonical/inherited coverage; no stale or incomplete automated evidence
reason remains.
