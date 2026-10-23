# Protected capture receipts and metadata-first block reads — 23 October 2026

Source checkpoint: `1bff36f`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

The final complete validation passed against matching clean source manifests:
**222 Rust tests**, **114 Android tests**, ten desktop JNI smoke groups, format,
strict Clippy, release, traceability, ARM64/ELF, Android lint, AAR payload and manifest
checks. Final runs have no failed/ignored/skipped tests. An earlier Android attempt
failed with a fixture-setup timeout; it and the isolated investigation are retained
below rather than silently replaced. Commands, hashes, timings and logs are archived
in `continuation-2026-10-23/capture-privacy/` and `latest/`.

## Verified defects and implementation

The prior receipt path compared fingerprints before reading the stored block under
current authority. Two new tests were run against that prior implementation and
failed: valid changed-payload retries could return Conflict while an identical retry
returned Denied for current block/exercise protection. That difference could confirm
a guess about protected original ink. Receipt retry now locates the stored block,
authorises its actual current owner/block/exercise scope, and only then fetches and
compares its stored fingerprint. Naming another public destination does not replace
stored authority. Authorised exact retries remain idempotent; authorised mismatches
still conflict. Source samples, hashes and existing receipt encodings are unchanged.

A separate synthetic working-row splice test also failed before the common guard:
ordinary reads trusted payload.entity without verifying its SQL-owner binding. Common
read_block now applies the existing metadata-only authority/owner-consistency check
before loading/deserialising block content, and validates payload.id against the row
key before return. CaptureReceipt benefits from that common guard as well. Corrupt
or spliced rows fail closed without silently repairing or reassigning protected data.
The working database is still not separately encrypted; no protection from arbitrary
OS-level database manipulation or constant-time execution is claimed.

Five new native fixtures cover matching/changed payloads and targets under block,
exercise and actual-owner protection, authorised exact/conflicting receipts, original
ink retention, protected malformed content without unauthorised deserialisation and
SQL/payload owner/ID mismatch. Existing capture/layer tests also passed. Actual JNI
coverage compares complete denied JSON replies (null data/generation), verifies fresh
proof restores correct replay/conflict behavior and retains exact source ID/negative
zero after rejected guesses. No private content is dumped in diagnostics.

## Validation attempts and remaining issue

The first complete Android attempt reached all 114 tests, with one failure:
PdfRetentionTest.lostPermissionAtFinalReopenReleasesNativeStageAndAllowsAnExplicitRetry.
Its XML stack locates a five-second CompletableFuture timeout at fixture's initial
Create command (line 64 via command line 33), before PDF selection/retention starts.
The failing case duration was 5.056 seconds. ARM64/ELF/JNI had passed. This attempt's
report/logs and failed XML are in `android-attempt-1/`.

The exact named case then passed independently on unchanged source: one test, zero
failures/errors/skips. Command and XML are in `android-isolated/`. The Rust validator
completed all 222 tests/release/traceability. A fresh complete Android validator run
on the same unchanged clean source then passed all 114 tests and remaining checks;
its reports are the main archive and latest artifacts. No timeout or assertion was
weakened, no test disabled, and tests were not aggregated across attempts.

**OPEN follow-up E01, within A7 boundary/lifecycle coverage:** investigate the initial
Create acknowledgement timeout under full validation. Cause is not established; later
passes do not prove a scheduling/load explanation or that a startup defect is fixed.
This remains a validation-confidence limitation, alongside full workflow/process/device
privacy acceptance. Other implementation gaps (including reviewed pending-layer
retargeting, complete authoring configuration, rendering/portability and recogniser
resource lifecycle) remain open. This increment does not establish whole-backend or
whole-boundary closure.

All eight gap groups remain OPEN. The gate exits 2 solely for those groups and
canonical/inherited coverage; no stale automated-evidence reason remains. The ledger
retains 94 requirements, 103 inherited clauses and 158 candidates without inferred PASS.
The handover folder is ignored, untouched and absent from tracked files/reachable path
history. Local commits only; nothing pushed. No production H.I., model, recogniser,
external inference or automatic OS backup change.
