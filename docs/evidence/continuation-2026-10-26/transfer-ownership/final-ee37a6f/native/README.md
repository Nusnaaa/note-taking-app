# Retained failed ee37a6f full native run

The workspace validation exited 101 during core library tests: `provisioning::import_control_tests::hash_loop_cancellation_preserves_an_unpublished_resumable_part` attempted mutable resume while retaining the original Transfer owner at provisioning.rs line 502, and received ResourceLimit. Format and Clippy passed. Core library results were 36 passed, 1 failed; later workspace suites, release-build and final traceability did not run. The report has no completed full-validation result.

The intended explicit handoff is to verify contention while the original owner is held, release that owner, and then resume/hash. Preserve this failure separately from its corrective follow-up. All eight backend gaps remain open.
