# Phase A — production backend build (in ChatGPT Work)

**Authorise via `01_START/WORK_START_PROMPT.md`.** This document describes the requested work; creation of the handover did not execute it.

## Working principle

Create one local, Git-tracked application project with a portable authoritative core and Android-facing adapter seams. Do not build production screens or Compose user journeys during Phase A. Command-line verification harnesses, synthetic fixture generators and automated test tools are allowed. Build functionality in cohesive, tested vertical **backend** increments rather than writing placeholder packages for the entire app. Production frontend is a separate **phase**, same repo.

## Required early actions

1. Inventory source documents and confirm approved v0.5.8 precedence. Inspect existing repository if present; never overwrite an uninspected repo or OS workspace.
2. Select a maintainable build/toolchain plan consistent with Kotlin Android, preferred Rust core, SQLite, with alternatives supported only if substantiated. Record versions and lock files; check local/Work machine capabilities before claiming Android execution.
3. Define first-release requirements ↔ implementation-component ↔ automated-check mapping; close `NB-SUP-001` and `NB-SUP-002` test TBDs; review 131+27 candidate test rows for duplicate/alias semantics. Do not treat an inherited count as automatically the executable total.
4. Create Git repository/branch safely, `.gitignore`, project-specific `AGENTS.md`, structured `docs/`, CI if available, tests committed in same meaningful incremental history as source. Maintain continuous progress/status in portable Markdown; no ZIP dumps as code reviews.

## Backend implementation order (engineering plan, not mandatory microservice split)

**A1: portable domain & organisation.** Durable stable IDs; Modules depth ≤2; legal parents; Project/Notebook/Note/Canvas; independent root content; link vs copy vs move; Trash/dependency warning payloads; permission checks; backlink indexes; chronology. Automated positive/negative, transaction/migration and referential-integrity tests.

**A2: source digital ink & document transactions.** Ordered original stroke coordinates/time/tool channels and provenance, derived render views separately, stroke region selection, editing history, durable atomic persistence, save interruption recovery, binary / corrupt input bounds, migration tests. Never replace original recorded strokes with smoothed/raster content.

**A3: editable content model & revision-safe proposal flow.** Structured maths presentation constructs, ambiguity/incomplete nodes, explicit code text with exact whitespace, prose/numeric/mixed regions, typed conversion alternatives and accept/correct/reject workflow. Recognition input snapshot and proposal carry revision/selection provenance; stale proposals cannot modify newer edits.

**A4: model/converter infrastructure.** Shared manager with specialist typed adapter interfaces; local capability registry, install/readiness states, profile, switch, cancel/timeouts, resource failures, explicit mode selection, mock engines, deterministic test doubles. Actual vendor selection conditional on licensing and compatibility. Do not implement optional generative M2 engine functionality just because the generic architecture can register that type.

**A5: PDF and external-source relations.** External reference default; optional explicit managed copy; permission/revocation/Q383 and historical snapshot rules; annotations and PDF-linked state as allocated; missing/relink flows; no arbitrary source ingestion. Test no implicit permission expansion.

**A6: import/export, protection and recovery.** Versioned portable native format; actual format converters only where feasible and licensed; fidelity/unsupported-section reports; comprehensive encrypted backup; backup/restore and transactional disaster tests; app/Note lock enforcement; protected-export reauthentication, encrypted-export portable credential, explicit unencrypted consent; Android OS backup exclusion prepared for later host configuration. Do not ship crypto invented from scratch.

**A7: integration contracts & hardening.** Stable Android/Kotlin host APIs / JNI or FFI safety, database migration, corrupt file handling, concurrent edits/model jobs, stress/restart/cancellation, storage/resource exhaustion and security/privacy negative tests. Developer harness for full synthetic authoring/export/restore lifecycle; test clean offline/non-network mode.

## Backend acceptance gate (must be demonstrated, not inferred)

Before opening Phase B, Work must provide a machine-verifiable **Backend Acceptance Report** with:
- exact source revision/commit, build commands, host/Android test environment and tool versions;
- matrix mapping every applicable I0/I1 backend requirement and inherited owner clause to tests or justified design evidence, with no silent unaddressed safety requirement;
- actual passing tests for storage/transactions/migration, legal hierarchy/identity, digital ink losslessness, recognition proposals, cancellation/staleness, adapter switching, PDF references/revocation, access/backup/export and boundary integration;
- failure-path tests and any skipped or platform-only tests honestly declared, including unresolved `NB-SUP-*` mappings if not closed;
- no secrets or user handwriting in fixtures/logs/commits;
- clear distinctions between passed backend contracts, pending hardware/device/real-engine acceptance, and release blockers;
- human-readable backend public API and integration guide for the future Android H.I.

**Do not begin production H.I. if the applicable backend contracts are not demonstrably complete and passing.** Fix backend defects rather than applying superficial green-test exceptions. The gate does **not** require owner to perform model/S Pen quality checks prematurely; those follow completed build. Unknown numeric SLOs are not a licence to fabricate thresholds.

## When to interrupt the owner

Only if the planned implementation would change a binding product behaviour, materially violate privacy/licensing, lose original data, need non-approved remote access, or require a genuinely consequential design fork. Ordinary library/version/test/algorithm selections may be made transparently with a recorded rationale. Keep safe unrelated development progressing.
