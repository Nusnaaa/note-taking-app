# Stored engine activation plans — component evidence

Based on pushed 37ecdfe. Source hashes include new then-uncommitted inputs. All eight backend gaps remain open.

`broad-tests.log`: `cargo test -p stem-core --test engine_activation_plan --test ready_runtime --test runtime_detachment --test activation_work --test host_activation --test host_recognition --test engines --test processing_work --test provisioning --test converters --locked --offline`, exit 0, 71 passed, zero failures/ignored. Five new plan cases verify probe re-entry without a live Manager borrow, durable publication/replay refusal, failed probe/SQL rollback with candidate retention and retry, stale generation/foreign library rejection, load/hash changes and converter cancellation/success.

`clippy.log`: workspace/all-target Clippy `--locked --offline -- -D warnings`, exit 0. Environment: CARGO_TARGET_DIR=/tmp/stem-pen-response-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2.

The initial check passed with unused exports before Store publicly re-exported the new plan types. Initial 29 selected cases passed with an unused fixture import; that import was removed and the resource chunk buffer made Zeroizing. Final broader tests and warnings-denied Clippy passed. Earlier results are retained, not substituted for final-source evidence. Synthetic models/resources are test fixtures, not actual recogniser quality, licence/device/SLO evidence.

The API separates metadata/probe/publication phases in native Store code. Host optional-worker/control/retirement adoption, Java providers and global controller/staged-resource policy integration remain open. No actual model is installed or enabled by this source addition.
