# Verification, traceability and practical completion gates

## Inputs and limitations

- 94 tracked canonical requirement texts present in approved cumulative v0.5.8; 103 inherited original clause texts present; **content presence is not complete semantic mapping**.
- 131 Stage5D atomic/alias test candidates plus 27 Stage5E supplementary candidates = **158 candidate rows**, not 158 distinct approved executable tests. All application tests still `NOT RUN` in source audit.
- **92/94** mapped to historical checks. Close `NB-SUP-001`, `NB-SUP-002` references listed `TBD`, with new/updated meaningful checks.
- **16 numerical SLO proposals unapproved**, no actual on-device measurements. Do not silently ratify or fabricate pass/fail thresholds.
- **14 engineering feasibility gates** have stage rephasing proposed to respect deferral of owner-local model/stylus evaluation. Test types: unit, property, integration, migration, negative/security, synthetic provider contract, Android automated, post-build manual owner.

## Mandatory engineering workflow

1. Review first-release requirements by area and inherited clauses. Map each to backend API(s), persistence/assets and tests, or to a reasoned post-build device/manual acceptance case.
2. Reconcile tests by semantics and phase: preserve historic IDs and parent↔alias provenance, combine duplicates only where outcomes genuinely overlap, avoid test-count inflation. Mark every carried-forward check with current `PASS`, `FAIL`, `NOT RUN`, `SKIPPED`, `BLOCKED` and evidence.
3. Write tests from the beginning and commit them together with feature increments (even if temporarily red). No binary ZIP-based test dumps or leaving tests untracked.
4. Use synthetic/public **stroke vectors** and mocks for recognition contract tests. Never require the owner's handwriting or raster renderings; don't fake model predictions as measured accuracy.
5. Test content ownership, transactional recovery, identity/containment, source revocation, safe exports, protected-content auth and backup restoration before the backend gate. Test negative cases (bad metadata, permission denied, corruption, cancelled jobs, stale revisions).
6. Account for split-phase features: backend invariants that protect exposed I1 resources are required now, while later M1/M2 sophisticated features are not prematurely implemented.
7. Produce honest backend acceptance evidence before any production H.I.; manual native S Pen/model quality remains `NOT RUN` until complete runnable build, and *must* be completed for eventual release acceptance.

## Minimum evidence deliverables

- Machine-readable requirements-to-code-to-test ledger and manual-test backlog, with phase, severity, owner requirement source, evidence location and result.
- Reproducible `build`, `test`, `lint`, `format`, `migration`, `test-offline` commands and environment versions.
- Backend Acceptance Report with observed results and known limitations; Android H.I. Gate Decision with explicit pass/fail status.
- Later: installable Android build with changelog, signed/developer build instructions where permissible, list of available installed recognisers/converters, owner-private manual checklist, actual observed issues and corrections.

## Never confuse

Passing a synthetic adapter-contract check ≠ an effective recogniser. Compiling code ≠ end-to-end acceptance. A Kotlin/JNI contract unit test ≠ measured Galaxy S Pen palm response. A lock icon ≠ separate database encryption. Previewable LaTeX text ≠ complete structured maths editing. A source distribution licence ≠ vendor weight redistribution rights.
