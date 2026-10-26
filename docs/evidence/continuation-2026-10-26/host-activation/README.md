# Host-owned activation — component evidence

Based on 9af24b7 (same implementation as d4b9615 plus evidence). Source hashes include the new then-uncommitted files. All eight backend gaps remain open.

`broad-tests.log`: `cargo test -p stem-core --test host_activation --test host_recognition --test activation_work --test host --test processing_work --test engines --test provisioning --test converters --locked --offline`, exit 0, 70 passed with zero failures/ignored. Seven new host cases verify current/foreign/app authority, pen priority and count admission, authoring/source preservation during a running probe, off-writer idle adapter cleanup, shutdown through both running probe and actual destruction before reopening, provider error cleanup, and descriptor-frame rejection preserving Ready/authoring.

`unit-tests.log`: `cargo test -p stem-core --lib host::recognition::tests --locked --offline`, exit 0, 4 passed, 27 unrelated filtered. Startup-worker failure now explicitly exercises activation reservation as well as recognition, preserving authoring. `clippy.log`: `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, exit 0. Environment: CARGO_TARGET_DIR=/tmp/stem-pen-response-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2.

Development failures retained: first check scoped two new helper methods too narrowly; visibility was corrected to crate::host, and final check passed before the later test fixtures were added. The first new fixture used a unit Relock constructor instead of Relock {}; fixed before the initial 48 selected tests passed. The later 70 integration and 4 scope tests plus Clippy cover the final source. Synthetic probe/cleanup gates are lifecycle evidence, not recognition quality or a device/SLO benchmark.

No callbacks are sent to the writer by this API. Resource/hash preparation and probing run on optional callers; host completion keeps provider/error holders until off-writer disposal. JNI providers, real recognisers, global model/controller retirement and hard worker isolation are separate open work.
