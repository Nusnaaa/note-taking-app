# Retained failed full native validation

Source d6051d4. `python3 tools/validate.py --output docs/evidence/continuation-2026-10-26/engine-removal/final-d6051d4/native` exited 101. Format and Clippy passed; the workspace test stage failed at `core/tests/engine_recovery.rs::recovery_requires_current_app_access_and_bounded_host_reservations`, line 267, because readmission immediately after dropping two plans returned ResourceLimit. The test assumed queued off-writer retirement had already released capacity. Scope retirement releases its reservation after actual payload cleanup; immediate readmission is not promised.

The full command stopped at that failure; release-build and final traceability stages did not run, and there is no complete all-tests success or finished report. Preserve this failure independently of follow-up verification. Test watchdogs and elapsed times are not product performance claims. All eight backend gaps remain open.
