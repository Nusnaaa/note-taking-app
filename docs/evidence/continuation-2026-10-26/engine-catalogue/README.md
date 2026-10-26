# Engine catalogue and durable defaults

`cargo test -p stem-core --test engines --test host_engine_activation_plan --test engine_activation_plan --locked --offline`: 38 passed, zero failed/ignored. `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo fmt --all -- --check`, `python3 tools/traceability.py --check`, and `git diff --check` passed. Cached offline Rust toolchain; CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, target /tmp/stem-activation-target, default debug profile.

Five new regression cases and extended reopen/access assertions cover keyset pagination and path omission; SQL engine binding integrity; converter rejection before default persistence; saved preferences on unsupported replacement and later compatible reload; and private Host readiness through close/reopen/explicit activation. The retained initial regression run failed two reload assertions because the direct activation API had its own publication path. Both activation paths now restore supported durable preferences.

These are synthetic lifecycle tests, not real model eligibility, recognition quality, device measurements, or full backend acceptance. JNI/Android catalogue adoption, recovery and the eight tracked backend gaps remain open. Full validation of this source will be recorded separately.
