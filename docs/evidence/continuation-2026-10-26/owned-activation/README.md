# Owned engine activation — component evidence

Based on pushed 4d9b70f. Source hashes identify the new then-uncommitted implementation/tests. All eight backend gaps remain open; no model installation, recogniser quality, JNI provider integration or hard worker-isolation claim.

`final-tests.log`: `cargo test -p stem-core --test activation_work --test processing_work --test engines --test recognition --test host_recognition --test provisioning --test converters --locked --offline`, exit 0, 59 passed, zero failures/ignored. Seven new activation cases cover blocked probes and manager progress, actual adapter retirement through removal, stale/foreign/replayed completion, cancellation/probe failure/panic, consent versus resource integrity, converter/abandoned work, provider-owned error destructor lifetime and original-error compatibility. Existing host/processing/provisioning/conversion/proposal regressions passed.

`final-clippy.log`: `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, exit 0. Environment: CARGO_TARGET_DIR=/tmp/stem-activation-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0. These isolated debug-information settings do not alter runtime security/KDF settings.

Development results retained: initial build failed with E0382 (moving self before linking its epoch); fixed by binding linked control before consuming work. The initial 29 cases then passed. Review added provider error-payload retention and its sixth case; 30 selected cases passed. The broader run passed 59 after adding original-error compatibility. First Clippy failed map_identity; `.copied()` replaced the redundant map, and final tests/Clippy passed. No earlier result substitutes for final-source evidence. Probe panic text is an expected synthetic test case and is contained.

Owned probes require optional worker execution and caller-owned off-lock/off-writer completion retirement. The completion method preserves resource/error holders; it does not provide a host worker queue or force a running provider to terminate. The synchronous compatibility API remains available under its earlier caller-owned rules.
