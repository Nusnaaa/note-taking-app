# Owned runtime detachment — component evidence

Based on pushed 0240da4; source hashes include the new then-uncommitted inputs. All eight backend gaps remain open.

`broad-tests.log`: `cargo test -p stem-core --test runtime_detachment --test host_activation --test host_recognition --test activation_work --test processing_work --test engines --test provisioning --test converters --locked --offline`, exit 0, 61 passed, zero failures/ignored. Five new core cases cover off-lock destructor re-entry, stale/default/inference/activation cancellation, converter compatibility and actual ResourceLease deletion refusal through a running worker. Three additional host cases cover pen-stale rejection, off-writer cleanup with ownership until close/reopen, and cleanup re-entry after controller unlock.

`unit-tests.log`: `cargo test -p stem-core --lib host::recognition::tests --locked --offline`, exit 0, 4 passed, 27 unrelated filtered. Startup failure now explicitly covers detachment reservation too. `clippy.log`: workspace/all-target Clippy with `--locked --offline -- -D warnings`, exit 0. Environment: CARGO_TARGET_DIR=/tmp/stem-eraser-batch-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0. Debug-information settings do not change runtime security parameters.

The first selected core run passed 30 tests before host detachment integration. The initial host check passed with an unread RAII-holder field warning; naming the resource-holding field `_runtime` records its disposal-only purpose. Final broad tests and warnings-denied Clippy passed. No earlier result substitutes for final-source evidence. Synthetic models/gates are lifecycle fixtures and make no recognition/device/SLO claim.

Logical detachment is not global resource retirement. The explicit running-worker resource test proves file deletion stays blocked after detached-holder disposal until the worker actually releases its lease. Durable/global uninstall and staged replacement controllers, Java providers and hard isolation remain open.
