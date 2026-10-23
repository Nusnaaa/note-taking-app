# Backend continuation — 23 October 2026

The owner requested continued complete I0+I1 backend implementation. The receiving
checkout initially contained only its placeholder README. The locally supplied
version-3 bundle was verified and imported without replacing the existing `main`
branch or configuring a new remote. The implementation continues on
`phase-a/backend` from `83b80b7f01c79ee59419cad1e5feb81cfbd90736`.
The existing origin is retained; no push has been performed.

The handover directory is excluded by the tracked root `.gitignore` and local
`.git/info/exclude`. Local pre-commit and pre-push hooks reject tracking it and
reject outgoing history containing it, including later-deleted files. Synthetic
throwaway-repository checks verified allowed clean commits/push history, rejected
staging and rejected contaminated history before/after deletion. The actual
outgoing branch also passes the local history guard. Hooks are local protections,
can be bypassed, and are not remote branch protection. No handover file is tracked.
The bundle/ZIP are inputs to recovery, not substitutes for unpacked committed source.

## Native host admission increment

Source checkpoint: `588d7d96456531c1d2004c855fa892be85becab3`.

- Independent authoring/query count and encoded-payload-byte budgets include queued
  and executing requests. Saturated optional reads cannot consume authoring admission.
- Capture, edits, undo and access changes preserve FIFO order and run ahead of queued
  reads. An already running operation is not preempted. Close rejects new admission
  and drains already admitted work; dropping a response receipt does not cancel it.
- Pull-based coalesced admission status contains only counters/flags, with no document
  identifiers, titles, content or ink. Observers share a bounded snapshot rather than
  event queues or writer callbacks. Failed admitted authoring/authentication leaves
  attention sticky for the host lifetime; this is not a per-document save receipt.
- Failed or panicking trusted authentication clears old grants and leaves the writer
  available. JNI clones each host under the handle-registry lock, releasing the lock
  before request completion, authentication callbacks or final host destruction.
- The new desktop JNI check blocks authentication on one library and verifies another
  library remains accessible. This does not establish Android device authentication.

The initial targeted run passed seven tests and failed one new fixture assertion
because it read the wrong tagged-content JSON field. Correcting the assertion to the
existing `content.value` representation yielded eight passing host tests. The strict
workspace Clippy check also passed. No production code was changed to satisfy that
incorrect assertion. The complete Rust validation passed format, strict workspace lint, all 138 tests
(with no failures/ignored tests), release build and traceability. The actual release
JNI library also passed all seven desktop JVM smoke groups under JDK 17 with
`-Xcheck:jni`, including independent registry access. The ARM64 native/ELF, desktop JNI, Android compilation, all ten Android unit/
Robolectric tests, lint, AAR payload and merged library manifest checks also passed.
Desktop and Android fingerprints match. This is component/backend-library evidence,
not device, hardware biometric, model-quality or full backend acceptance.

Validation outputs for this increment are under
`continuation-2026-10-23/native-admission/`. The runner fingerprints all tracked
non-evidence inputs and records the source commit, commands, logs and environment.
Toolchain downloads use upstream URLs/checksums, stay under `/tmp`, and are not
committed: Rust 1.90.0, JDK 17, Gradle 8.9, Android SDK/build tools 35 and NDK
27.0.12077973. Only synthetic application fixtures and build dependencies are used.

All eight backend gap groups remain OPEN. Android admission/priority scheduling,
active-stroke interruption/recovery, Android subscription integration and per-ticket
capture receipts are still implementation work. Original ink remains independent of
recognition and history; no handwriting upload, raster recognition path, remote
inference, model weights or production H.I. has been introduced. Backend acceptance
remains NOT MET. Test counts do not close complete requirements or hardware checks.

The concurrent scratch Gradle build exposed an implicit Build Tools 34 auto-install
race in the shared SDK. The authoritative baseline run completed with ten passing
Android tests, while the scratch attempt failed on the damaged SDK package. The
scratch retry explicitly pins Build Tools 35 (matching the documented toolchain);
the incomplete SDK package is repaired from the official checksummed archive.
No failed scratch attempt is counted as passing application evidence.
