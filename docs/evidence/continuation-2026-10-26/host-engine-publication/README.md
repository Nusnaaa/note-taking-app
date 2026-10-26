Host stored-engine publication
==============================

Verification: cargo test -p stem-core --test host_engine_activation_plan --test engine_activation_plan --test host_activation --test host_recognition --test runtime_detachment --locked --offline: 49 passed (including six new publication cases); cargo clippy --workspace --all-targets --locked --offline -- -D warnings: passed. cargo fmt --all -- --check: passed. Target /tmp/stem-host-engine-plan-target; CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, cached offline toolchain. The invalid-target log retains an invocation error, not a test result.

Synthetic provider cases establish host publication, durable installation, replay/foreign/stale/capture rejection, busy-manager retry, off-writer old-runtime destruction and ownership through retirement, and writer authentication reentry. No real model, device performance, global memory isolation or full backend completion is inferred. All eight backend gaps remain open. Full current-source native/Android validation is a separate pending step.
