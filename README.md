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
| `core/src/portable.rs`, `attachments.rs` | Native v3 subset with rich attachments and independent identity remapping |
| `core/src/backup.rs`, `crypto.rs` | Encrypted whole-library snapshot and separate-library recovery |
| `core/src/export.rs` | Scoped converted-only Markdown/SVG and indivisible-object pagination |
| `core/tests/` | Synthetic unit/integration, negative, migration and recovery tests |
| `core/src/bin/recovery_probe.rs` | Developer-only abrupt-process-exit test helper |
| `docs/verification/` | Current requirement mapping, deferred manual checks and open acceptance work |
| `docs/evidence/` | Import provenance and observed validation evidence |

No real recogniser is installed. Synthetic adapters live in tests and make no
handwriting-accuracy claim. No handwriting uploads, raster recognition requirement,
remote inference, LLM, telemetry or production H.I. have been introduced.

The core now stores shared exercise views, structured tables, diagrams, graph data,
image assets, groups, layouts and linked canvas cameras. Observed-input routing,
bounded capture queues with durable retry receipts, cooperative priority and a
physical reserve-file primitive are implemented. Local recogniser staging records
consent and provenance; resource integrity and a real adapter probe precede activation.
Failed updates preserve the previous adapter. No production recogniser is bundled.

PDF source revisions require review, preserve authored work and mark old anchors for
reconnection. Native subset v3 includes rich assets and view relationships and reads
v1/v2. The separate encrypted whole-library backup includes retained history, Trash,
settings, proposals and authorised source snapshots. Restore creates a new library,
preserves object identities within it, clears provider availability and excludes model
files. Converted-only derivative exports do not obtain original strokes unless
explicitly requested. See the format documents and ADR-004 for exact boundaries.

Remaining work includes Android backend bindings/security/input/provider integration,
a complete processing lifecycle, region split/merge, general undo/redo, full rendering
and paginated PDF export, larger-library backup handling and acceptance coverage.
The new primitives still require host orchestration. These are implemented/tested
core components, not a runnable app or complete backend acceptance.
See `docs/verification/backend-gaps.json` and `docs/evidence/Backend_Progress_Report.md`.

## Repository and future host

Branch: `phase-a/backend`. This is a local Git repository with individual readable
source, tests and documentation. No remote has been selected or pushed. A private
remote can be connected later using its explicitly authorised URL. A Git bundle,
when supplied alongside the readable files, preserves the actual commit graph:

```sh
git clone STEM_Notebook_Phase_A.git.bundle stem-notebook
cd stem-notebook
git switch phase-a/backend
```

Kotlin Android bindings and the eventual Compose/native-ink host belong in this
same repository. Production H.I. implementation starts only after backend
acceptance. Owner-local model, pen and converter testing stays deferred until a
complete runnable app exists; it is still required for eventual release sign-off.

This is a personal, non-commercial project. No public licence has been assigned to
the owner's source. Dependency usage and source/binary redistribution are recorded
separately in `docs/licensing/DEPENDENCIES.md`; proprietary weights, credentials,
private notes and third-party binaries are not committed.
