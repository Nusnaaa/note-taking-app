# Ready-runtime handoff — component evidence

Based on pushed b9a407b. Source fingerprints include the new then-uncommitted inputs. All eight backend gaps remain open.

`broad-tests.log`: `cargo test -p stem-core --test ready_runtime --test runtime_detachment --test activation_work --test host_activation --test host_recognition --test processing_work --test engines --test provisioning --test converters --locked --offline`, exit 0, 66 passed with zero failures/ignored. Five new handoff cases cover failed/unready candidates preserving the old engine, old-work invalidation with resource pinning, frozen/abandoned candidate disposal, family mismatch/default compatibility and probe/destructor re-entry into the existing registry after unlock.

`clippy.log`: workspace/all-target Clippy `--locked --offline -- -D warnings`, exit 0. Environment: CARGO_TARGET_DIR=/tmp/stem-activation-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0. Debug-information settings change no runtime KDF/security parameters.

The initial selected run passed 28 cases but reported an unused test import; initial Clippy rejected that import. It was removed, and the final broader run plus Clippy passed. The check log predates the new fixtures and is not substituted for final-source coverage. Synthetic adapters are lifecycle fixtures and assert no actual model quality, licensing permission, device performance or SLO.

The public primitive moves an already probed in-memory runtime. Durable installation/owner-review checks, host update capabilities, reviewed destination configuration fencing and controller adoption remain open. Outstanding old workers keep their independent leases; returned old-holder disposal is not a global resource-uninstall acknowledgement.
