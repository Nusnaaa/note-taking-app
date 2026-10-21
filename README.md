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
| `core/src/store/`, `core/migrations/` | Transactional organisation, source ink, editable blocks, history and proposals |
| `core/src/content.rs`, `ink.rs` | Editable presentation structure and original vector observations |
| `core/src/processing.rs`, `converters.rs` | Shared manager with separate typed local recogniser/converter contracts |
| `core/src/security.rs`, `sources.rs` | Scoped access and local PDF source/retention policy |
| `core/src/portable.rs`, `crypto.rs` | Explicitly limited native subset and portable encrypted envelope |
| `core/tests/` | Synthetic unit/integration, negative, migration and recovery tests |
| `core/src/bin/recovery_probe.rs` | Developer-only abrupt-process-exit test helper |
| `docs/verification/` | Current requirement mapping, deferred manual checks and open acceptance work |
| `docs/evidence/` | Import provenance and observed validation evidence |

No real recogniser is installed. Synthetic adapters live in tests and make no
handwriting-accuracy claim. No handwriting uploads, raster recognition requirement,
remote inference, LLM, telemetry or production H.I. have been introduced.

The next implementation work is shared master/exercise views, rich technical
objects, durable processing lifecycle, full portability, PDF host integration and
Android core bindings. See `docs/verification/backend-gaps.json` for gate-blocking
work and `docs/verification/TEST_PLAN.md` for verification responsibilities.

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
