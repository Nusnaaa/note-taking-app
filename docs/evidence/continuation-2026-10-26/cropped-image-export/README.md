# Cropped-image derivative export — component evidence

Based on pushed f9e7ad0, with source fingerprints covering the new then-uncommitted files. All eight backend gaps remain open.

`final-tests.log`: `cargo test -p stem-core --test image_crop_export --test export --locked --offline`, exit 0, 10 passed (5 new crop cases and 5 existing derivative cases), zero failures/ignored. `clippy.log`: `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, exit 0. Environment: CARGO_TARGET_DIR=/tmp/stem-eraser-batch-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0. Isolated debug-information settings change no runtime security/KDF settings.

The first offline test invocation refreshed Cargo.lock to add only stem-core's direct edge to the already locked image 0.25.10 package; no package version/checksum changed and no dependency download was required. It passed the same ten cases with three unused-mut fixture warnings. Those warnings were removed and the metadata exclusion assertion now uses the actual marker length; final tests and warnings-denied Clippy passed.

Tests decode emitted PNGs and inspect the selected pixels/dimensions, absence of original trailing metadata, distinct outputs for two crops of one asset, original asset byte preservation, explicitly included original ink, inward rounding/empty-pixel review, PNG/JPEG support, EXIF rotation review, malformed/oversized input and current block protection. These are component tests, not device performance, full PDF/scene fidelity or a hard total codec allocation/isolation guarantee.
