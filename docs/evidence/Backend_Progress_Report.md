# Phase A implementation progress and backend gate

22 October 2026 (Europe/London). **Backend acceptance: NOT MET — incomplete.**
Production Android H.I. remains closed. This report records a real implementation
increment and executed checks; it is not acceptance of the full I0+I1 product.

## Actual repository and implementation

The handover was extracted and its 36 manifest-listed content members verified.
`START_HERE.md`, current authority, the start prompt, approved v0.5.8 and the
implementation/verification instructions controlled the work. The approved source
is preserved byte-for-byte (SHA-256
`68612d4dc4d0e69c61f0018fbcf67bf779559bcbb120bcd0f6c3f9333f27b7b3`).
Twenty-six selected authoritative source files were imported individually with
their hashes; the ZIP was not committed.

Created the local `stem-notebook` Git repository and `phase-a/backend` branch. No
remote was selected or pushed. Implemented Rust/SQLite source and synthetic tests:

- Legal Module/Project/Notebook/Note/Canvas containment, stable IDs, Unfiled,
  transactional moves, chronology, basic links/backlinks, deletion review and Trash.
- Immutable ordered vector observations, nullable pressure/tilt channels, derived
  transforms/erasure, editable text/numerals/code/maths presentation trees, local
  subtree correction, display state and active-use history retention.
- Separate typed recogniser/converter interfaces under one local manager. Revision-
  bound proposals, correction/rejection/explicit atomic acceptance, compatible model
  switching and failure/cancellation paths. Test doubles are not real recognition.
- Backend scoped access checks, literal accepted-content/title search and offline
  recovery Help. Fresh protected-export authentication on both output branches.
- PDF source identity/relink/view metadata, explicit managed-copy consent, cache
  revocation and independently authorised historical-snapshot retention rules.
- Documented native **subset** export/import with independent restored IDs,
  Argon2id/XChaCha20-Poly1305 password and independent recovery-key opening,
  corruption rejection and current-data preservation on failed import.
- SQLite migrations, expected-revision conflicts, process-exit recovery and actual
  SQLITE_FULL rollback tests, plus reproducible validation and gate scripts.

No production H.I., real model, Android build, full backup or complete PDF renderer
is claimed. No user handwriting or handwriting image was requested or transmitted.
All ink fixtures are synthetic vectors. No raster-recognition dependency, Pix2Text,
remote inference, LLM or model telemetry was introduced.

## Exact executed evidence

Tested source commit: **`a0fce748bea5e76afb16b24b26187176f2d0039c`**.
Working tree was clean at the start. Subsequent evidence commit adds this report
and logs only; it does not change tested implementation or test inputs.

Executed `python3 tools/validate.py`. Its observed outputs are retained in
`latest/validation.json` and the named logs, including source SHA-256 values.

| Command | Observed result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS, exit 0 |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | PASS, exit 0 |
| `cargo test --workspace --locked --offline` | **53 passed, 0 failed, 0 ignored**, exit 0 |
| `cargo build --workspace --release --locked --offline` | PASS, exit 0; Linux x86_64 build |
| `python3 tools/traceability.py --check` | PASS, exit 0; 94/103/158 source rows retained |
| `python3 tools/backend_gate.py` | **NOT MET**, intentional exit 2; implementation/coverage gaps remain |

Test groups: storage unit 1; converters/Help 4; organisation 9; portable export/
crypto/import 10; recognition contracts 10; migration/process/concurrency recovery 4;
source policy 4; writing/history/search 11. Doc-test and helper-binary targets had
zero tests. Deferred manual/device checks were **not run**, not counted as passes
or as ignored Rust tests. Execution timings are runner observations, not SLO claims.

Tests demonstrate actual committed SQLite recovery, rollback, state preservation,
source retention, scope denial, fresh authentication, tamper rejection and independent
subset restoration on this host. They do not demonstrate physical power-loss
behaviour, device latency, real recogniser quality, OS egress isolation, cryptographic
audit or completeness of the product.

During implementation a portable-export type-inference error and a Clippy API-width
failure were corrected. Hardening activation state changed integrity failure from
Missing to Failed; an old assertion consequently failed and was updated to the
intended honest readiness state. The affected 20 tests then passed, followed by the
full 53-test clean run above. These earlier failures are not concealed by the final
result; the authoritative complete-run logs are those in `latest/`.

## Commit history

| Commit | Actual change |
| --- | --- |
| `d3c3478` | Import approved baseline, owner authority and verified provenance |
| `c6ca828` | Transactional organisation, lossless ink and executable regression tests |
| `284e1c4` | Reviewed recognition proposals and permission-scoped PDF sources |
| `5d8fc0b` | Encrypted portable subset, converters and failure/recovery tests |
| `84b9489` | Reject cross-kind import ID aliases; fail closed on activation integrity/probe failure |
| `ec61644` | Current requirement map, documented boundaries, licences and reproducible backend gate |
| `a222bb3` | First complete 53-test evidence snapshot and explicit incomplete backend gate |
| `a0fce74` | Record command/exit metadata for successful silent checks; validated again with all 53 tests passing |

The final evidence-only commit contains this updated report and final command/output logs. Use
`git log --oneline` for its exact ID; the code revision tested remains the one above.
All commits contain individual source/test/documentation files. A separately
provided Git bundle preserves the commit graph and is not committed as a source
substitute. The source tree is also supplied as individual readable files.

## Requirement coverage and remaining acceptance work

The current machine-readable map preserves 94 canonical requirements, 103 inherited
clauses and all 158 candidate assertions. Overlapping fixtures are grouped without
inflating test counts or discarding different outcomes. Search (`NB-SUP-001`) and
Help (`NB-SUP-002`) now have explicit executable references; their full requirements
remain partial. Two inherited editorial cross-reference errors and the SV-16 phase
mismatch are documented without editing the approved source. Semantic completion
of the whole baseline is not certified by row counts or individual passing tests.

| Gate-blocking area | Consequential unfinished work |
| --- | --- |
| Shared exercise model | Master/linked view membership, heading-safe reassignment, linked save state and copy semantics |
| Native capture host | Palm/touch cancellation contract, priority scheduling, persistence queue, low-storage reserve and tool/view persistence |
| Rich technical objects | Tables, geometry, graphs, images, groups, canvas cameras/embeds, layout and region/batch acceptance |
| Model lifecycle | Durable profiles/install/update staging, resource leases, bounded workers and an eligible actual recogniser integration |
| PDF integration | Platform provider/rendering, anchors/annotations, permission reacquisition and reviewed replacement of changed revisions |
| Complete portability | Full source/assets/settings/proposals/views/Trash roundtrip, import lineage, global undo/redo, comprehensive encrypted backup and paginated PDF/converted-only export |
| Android boundary | Kotlin bindings/ABI build, actual authentication, OS cloud-backup and device-transfer exclusions, diagnostics and process isolation |
| Coverage closure | All applicable clause assertions, remaining interrupt/corruption/resource/scale/fuzz cases and later owner-private device testing |

Current content locks are authorisation checks, not independently encrypted database
storage. The native subset refuses source-linked documents and is **not** a full
backup. Current model cancellation discards late results but cannot terminate a
non-cooperative native worker. These limitations are explicit acceptance gaps.

## Environment, licensing and next execution

Rust was initially absent; the official rustup toolchain was installed. Observed
versions: Rust/Cargo 1.90.0, GCC 13.3.0, Python 3.12.14 and Git 2.51.1. Java 17 runtime
is present, but no javac, Kotlin compiler, Gradle, Android SDK/adb/NDK toolchain is
configured. Therefore Android automated compilation/integration cannot yet be
reported. `environment.json` retains the actual inventory commands and output.

No owner decision currently prevents further portable-core coding. Remaining work
is implementation and validation, not a request to redesign the specification.
Android tooling must be provisioned for host validation. Actual model candidates
need separately evidenced personal-use rights, stroke-native/offline compatibility
and integration; private quality testing waits for the runnable application.

The 83 pinned registry dependencies have a licence declaration/file-hash inventory;
personal use, the owner's original-source publication and third-party redistribution
are treated separately. No proprietary SDK/weight, secret or private note is in Git.
This is not a completed binary-notice or dependency-vulnerability audit.

Resume with the open groups in `docs/verification/backend-gaps.json`, beginning with
shared exercise identity and lock semantics. Keep all work in this same repository,
run the documented checks after feature increments, then produce a new full Backend
Acceptance Report only when the complete gate is met. Preserve all 16 numerical SLOs
as unapproved and all owner-local manual checks as deferred until the runnable app.
