# Exact-source independent-root native validation

`python3 tools/validate.py` completed with exit 0 against pushed 3b9b0a0. Formatting, Clippy, all 335 native tests (zero failed/ignored), release build and generated traceability check passed. The native and Android reports have identical 273-file source manifests, all matching the checkout at archive time. The same source separately passed 339 Android tests and all 28 normal/R8 JNI groups, ARM64, AAR packaging, manifests and lint. The earlier fcc0716 stale-map failure remains preserved; this run passes its corrected generated maps.

The backend gate still exits 2 solely for the eight open implementation gaps and incomplete requirement/inherited-clause coverage. It does not fail for stale source, incomplete automated checks or platform/source mismatch. Individual passing tests are not complete acceptance of all 94 requirements, 103 inherited clauses or 158 candidate assertions. Production H.I. remains gated; actual device/Keystore/provider/boot/ART acceptance is not inferred.

Commands, environment, exact source hashes, timings and original output remain in the reports/logs. The pre-existing owner host-restore evidence directory was untouched. Ongoing inactive-route retirement work is isolated and outside this verified source.
