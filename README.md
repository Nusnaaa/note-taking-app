# Mathematical Handwriting & Digital STEM Notebook

Phase A implementation is underway. This repository contains a working Rust/SQLite
core and executable automated tests. It is **not yet the complete I0+I1 backend**
and is not a runnable Android notebook. The backend acceptance gate is closed.

The controlling product baseline is `docs/spec/approved-v0.5.8.md`, together with
the later owner authority in `docs/imported/02_GOVERNANCE/`. The historical review
header and old CSV approval labels do not revoke the owner's subsequent approval.

## Build and validate

Install Rust with rustup; the repository pins Rust 1.90.0 and its lint/format tools.
A C compiler is needed for bundled SQLite. Python 3 runs the evidence tools.
PDF tests need DejaVu Sans or a local font supplied through `STEM_TEST_FONT`.
See `docs/verification/HOST_INTEGRATION.md` for the CLI and Android build instructions.

```sh
cargo fetch --locked
python3 tools/validate.py
python3 tools/backend_gate.py
```

The validation runner executes format, lint, tests, a release build and traceability
checks with Cargo's dependency access offline after the initial fetch. It records
actual commands, output, source hashes and the tested commit in `docs/evidence/latest/`.
The gate intentionally exits **2** until the complete backend is accepted. A green
test suite alone cannot open it. Individual checks can be run with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline
cargo build --workspace --release --locked --offline
python3 tools/traceability.py --check
```

## Source layout

| Path | Responsibility |
| --- | --- |
| `core/src/store/`, `core/migrations/` | Transactional organisation, shared exercises, source ink, editable blocks, history and proposals |
| `core/src/content.rs`, `objects.rs`, `ink.rs` | Editable text/maths/code, technical objects and original vector observations |
| `core/src/capture.rs` | Observed pen routing, bounded persistence admission, retry receipts and cooperative priority |
| `core/src/processing.rs`, `store/engines.rs`, `converters.rs` | Typed processing, durable local recogniser profiles and verified activation |
| `core/src/security.rs`, `sources.rs` | Scoped access and local PDF source/retention policy |
| `core/src/portable.rs`, `attachments.rs` | Native v4 subset with rich attachments and independent identity remapping |
| `core/src/backup.rs`, `crypto.rs` | Encrypted whole-library snapshot and separate-library recovery |
| `core/src/export.rs`, `render.rs` | Scoped converted-only Markdown/SVG and paginated Unicode/vector-maths PDF |
| `core/src/host.rs`, `bridge/`, `android/` | Versioned asynchronous host, JNI/ARM64 library, Android input, recovery and platform adapters |
| `core/tests/` | Synthetic unit/integration, negative, migration and recovery tests |
| `core/src/bin/recovery_probe.rs` | Developer-only abrupt-process-exit test helper |
| `docs/verification/` | Current requirement mapping, deferred manual checks and open acceptance work |
| `docs/evidence/` | Import provenance and observed validation evidence |

No real recogniser is installed. Synthetic adapters live in tests and make no
handwriting-accuracy claim. No handwriting uploads, raster recognition requirement,
remote inference, LLM, telemetry or production H.I. have been introduced.

The core includes shared exercises, technical objects, immutable source ink, region
partition/split/merge and revision-safe selected-region conversion. Durable authoring
undo/redo covers blocks, layouts, groups and embeds. Schema-9 whole-library backups
retain this journal; schema-8 backups restore with empty authoring undo history.
Native subset v4 carries regions and rich attachments and reads v1–v3.

The versioned host exposes authoring/navigation/object operations through a bounded
single-writer thread. The Android backend library adds JNI, observed MotionEvents,
vector capture encoding, a durable retry journal, low-space reserve release, trusted
platform authentication and read-only PDF provider/isolated parser components. It has
no Activity or production H.I. The library's manifest excludes OS backup/device transfer
and INTERNET permission; final app and device enforcement still need verification.

The Android backend also owns encrypted backup publication and separate-library
restore workers, including input grants, cancellation retirement and explicit
recovered-root discovery/opening. Orphan cleanup, hard staging disk quotas and
real-provider/device acceptance remain open.

Converted-only PDF output embeds selectable text and vector maths using a caller-supplied
local font. Unsupported glyphs/images/embed fidelity fail explicitly. Resumable consented
local resources and durable converter activation supplement the existing engine ledger.
No real recogniser is bundled. There is no handwriting upload or raster recognition path.

The complete I0+I1 backend is still unfinished. Remaining work includes the real local
recogniser/runtime integration, full canvas/export fidelity, PDF client and worker
orchestration, scalable/chunked backup, full operation history, caret/selection state,
remaining Android lifecycle integration and requirement acceptance. Passing the component
suites does not close these gaps. See `docs/verification/backend-gaps.json` and the actual
results in `docs/evidence/Backend_Progress_Report.md`.

## Repository and future host

Branch: `phase-a/backend`. Source, tests and documentation are kept as individual
readable files. The owner authorised periodic pushes to the configured origin,
`https://github.com/Nusnaaa/note-taking-app.git`; the backend branch was first pushed
on 2026-10-25. A Git bundle, when supplied alongside the readable files, preserves
the actual commit graph:

```sh
git clone STEM_Notebook_Phase_A.git.bundle stem-notebook
cd stem-notebook
git switch phase-a/backend
```

The Kotlin Android backend library is in this repository; the eventual
Compose/native-ink host will follow it. Production H.I. implementation starts only after backend
acceptance. Owner-local model, pen and converter testing stays deferred until a
complete runnable app exists; it is still required for eventual release sign-off.

This is a personal, non-commercial project. No public licence has been assigned to
the owner's source. Dependency usage and source/binary redistribution are recorded
separately in `docs/licensing/DEPENDENCIES.md`; proprietary weights, credentials,
private notes and third-party binaries are not committed.
