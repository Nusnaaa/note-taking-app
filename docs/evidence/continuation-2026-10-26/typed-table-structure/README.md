# Typed-table structural edit component checks

Commands executed in `/tmp/stem-engine-catalogue-work` with PATH=/tmp/stem-cargo/bin:$PATH, CARGO_HOME=/tmp/stem-cargo, RUSTUP_HOME=/tmp/stem-rustup, CARGO_TARGET_DIR=/tmp/stem-pen-response-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2 and RUST_TEST_THREADS=2 for tests.

- `cargo test -p stem-core --test table_cell_ink`: 25 passed, 0 failed/ignored. Includes insertion/removal/header/bounds, detached validation, Store CAS/Undo/Redo, commit failure rollback, fresh lock refusal, ink refusal and actual host writer command/retry checks.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
- `cargo fmt --all -- --check`: exit 0.
- `python3 tools/traceability.py --check`: exit 0, 94 requirements/103 inherited clauses/158 candidates retained; no acceptance inferred.
- `git diff --check`: exit 0 before evidence copy (raw compiler logs retain original formatting).

Initial 23-case pass is retained separately. Adding the host test initially failed to compile because Host::open takes an owned PathBuf; the original failure log is retained. Corrected fixture and all 25 cases then passed. No production implementation failure was concealed.

These are native component checks, not full workspace test acceptance, Android/physical device tests or completion of Q071. Full native and Android validations of prior source 81d22e3 run independently; they cannot establish this newer source. All backend gaps remain open. Ink-aware layout, resize and cell move/copy still require implementation (ADR042).
