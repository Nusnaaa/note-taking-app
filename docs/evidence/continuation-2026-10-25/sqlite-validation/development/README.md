# SQLite whole-library validator development checks

Environment: PATH=/tmp/stem-cargo/bin:$PATH, CARGO_HOME=/tmp/stem-cargo,
RUSTUP_HOME=/tmp/stem-rustup, CARGO_BUILD_JOBS=2.

- `cargo test -p stem-core --lib backup::validation::tests`: 8 passed,
  no failures/ignored. Counts represent test functions; the differential mutation
  cases are assertions within one function, not extra reported tests.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  exit 0.
- `cargo fmt --all`: exit 0.
- `python3 tools/traceability.py --check`: exit 0; all requirement/candidate
  acceptance states remain partial/not run.
- `git diff --check`: exit 0 for implementation/docs before staging evidence.

An earlier development build of the added large-image fixture failed because its
writable test BLOB handle lacked `mut`; corrected before the passing run. The first
four validator tests passed before the scale/depth/image fixtures were added; a
subsequent seven-test run passed before the final ancestry/history coverage and
additional cancellation polling. Only the final eight-test output is retained as
the current targeted execution record. These commands are not an entire-workspace,
Android or device acceptance result. Full native and Android/JNI checks follow on
the committed implementation snapshot.
