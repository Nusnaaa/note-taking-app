# Explicit owner release after cancelled hash verification

The ee37a6f full native command failed at the core hash-cancellation unit case because it kept its original mutable Transfer and attempted a second resume. Its full failed run is retained in ../transfer-ownership/final-ee37a6f/native. The fixture now asserts that competing resume is rejected while the original owner is held, drops that owner, then verifies resumption and the final hash. Production behavior is unchanged.

`cargo test -p stem-core --lib --locked --offline` with RUST_TEST_THREADS=2 passed all 37 core library tests, zero failed/ignored. `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo fmt --all -- --check`, `python3 tools/traceability.py --check`, and `git diff --check` passed. Offline cache, default debug, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, /tmp/stem-pen-response-target. Full workspace validation remains separate. All eight backend gaps remain open.
