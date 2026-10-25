# Intermediate validation snapshot 20a24f1

These outputs validate the intermediate SQLite-backed graph validator before the
prepared incremental edge-table and legacy cancellation corrections. They must
not be claimed as exact-source validation of those later corrections.
Android: 207 tests, no failures/errors/skips; 26 regular and optimized JNI groups;
ARM64/ELF/AAR/consumer-rule/manifest and lint checks pass. Native full validation completed: 324 tests passed, none failed/ignored; format,
workspace/all-target clippy, release build and traceability passed. Both full
reports identify the same clean implementation commit and matching source
fingerprints. backend_gate.py exits 2 because all eight backend gaps and complete
requirement/inherited-clause coverage remain open, without stale/mismatch reasons.
No device or physical failure/durability acceptance is claimed.
