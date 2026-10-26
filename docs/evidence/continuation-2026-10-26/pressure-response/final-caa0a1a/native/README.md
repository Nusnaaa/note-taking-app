# Exact caa0a1a native validator — failed run retained

`tools/validate.py` passed formatting and Clippy, then the workspace test invocation failed in `host_backup::close_waits_for_actual_sink_retirement_and_preserves_exclusive_library_ownership`: the ten-second synthetic sink-start channel wait timed out. The same run had ten passing host_backup cases, including the above-legacy-size case; no full workspace pass, release-build or traceability success is inferred.

`backup-timeout-followup.log` records the unchanged exact test passing with `cargo test -p stem-core --test host_backup close_waits_for_actual_sink_retirement_and_preserves_exclusive_library_ownership --locked --offline -- --exact --test-threads=1`, exit 0, one passed, 4.70 seconds. The initial cause remains unestablished. Concurrent validators were active; this is context, not a proven cause. The failing run and follow-up are retained without rewriting the failure.
