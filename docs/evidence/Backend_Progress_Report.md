# Phase A progress — capture, technical objects and library recovery

22 October 2026. **Backend/core implementation: approximately 60% complete,
40% remaining. Backend acceptance: NOT MET. Android H.I.: 0%.**

The percentage is an engineering estimate of implemented versus remaining I0+I1
backend work. It is not a measured fraction of passed requirements, a performance
score or permission to begin production H.I. The estimate reflects substantial
implemented storage, capture, object, processing, source and recovery components;
the remaining work includes consequential host integration and feature gaps below.
This is not yet a runnable Android notebook.

Validated source: `b077e2c143adc58931be5ef811aefa8d60e26924`, branch `phase-a/backend`.
The working tree was clean when validation began. This report and the logs are
committed afterwards as evidence-only changes. The previous 72-test checkpoint
remains in Git at `3369469`.

## Actual implementation in this increment

| Area | Implemented and exercised |
| --- | --- |
| Ink capture | Classified observed-event router, palm/touch/stylus policy, cancellation, bounded queue, failed-save retry and atomic idempotency receipts surviving reopen |
| Scheduling/recovery primitives | Cooperative background admission/cancellation, reservations retained until lease release, physically written/synced emergency reserve file |
| Technical objects | Editable table cells, diagram parts, graph axes/ordered series, local image bytes and integrity, atomic grouped transforms and explicit layouts |
| Views | Linked canvas cameras, independent frame/pin/duplicate state, cycle rejection, protected placeholders and persistent camera/tool/block-anchor settings |
| Local engines | Consented resource staging, bounded streaming/hash verification, durable versions/defaults, provisional probe before activation, rollback and removal; concurrent changes invalidate activation |
| Recognition review | Selected batch acceptance with stale-source checks and transaction rollback; unselected proposals remain pending |
| PDF metadata | Authored annotation anchors, reviewed revision changes, historical/pending-reconnection states, cache invalidation and explicit new-grant same-revision reacquisition |
| Portability | Native subset v3 rich attachments and independent IDs, with v1/v2 reads; library-bound export review |
| Library backup | Encrypted whole-library snapshot, password/recovery restore, Trash/history/proposals/settings/profiles/membership undo, authorised source snapshots and new-path-only publication |
| Derivatives | Converted-only scoped Markdown/SVG, deliberate optional original ink, fresh protected authentication, scope/hidden-point checks and indivisible-object pagination |

The combined restoration fixture covers technical graph data, local images, layouts,
groups, linked embeds, saved views, PDF anchors, pending proposals, engine profiles,
settings and capture retry receipts. Restoring model provenance does not establish
runtime readiness or copy model resource files. Source grants are not restored;
Available becomes Missing and Revoked stays Revoked. Restored objects retain identity
within a new library. The live library is untouched.

Earlier organisation, chronology/backlinks, source-preserving ink, editable maths/code,
shared exercises, literal search, history, scoped access, converters and help remain.
The current commit contains **113 individual tracked files**, **9,100 physical Rust
implementation lines**, **3,703 Rust test/fixture lines** and eight SQL migrations.
These counts include formatting and fixtures; they do not measure completion.
No ZIP archive is committed as source.

## Executed validation

The clean `python3 tools/validate.py` run was **09:45:27–09:45:59 UTC**. Its exact
commands, exit codes, output, commit and source hashes are in `latest/validation.json`
and adjacent logs. All Cargo validation uses the unchanged lockfile offline after
fetching dependencies.

| Check | Observed result |
| --- | --- |
| Rust formatting | PASS, exit 0 |
| Clippy, all targets, warnings denied | PASS, exit 0 |
| Full workspace tests | **108 passed, 0 failed, 0 ignored**, exit 0 |
| Release build | PASS, exit 0 |
| Traceability consistency | PASS, exit 0 |
| Complete backend acceptance | **NOT MET**, expected exit 2 |

Suite counts: SQLite-full unit 1; backup 7; capture 7; converters/help 4; durable
engines/batch proposals 6; exercise migrations 2; exercises 17; export 5; objects 8;
organisation 9; PDF revisions 3; portability/encryption 10; recognition 10;
recovery/concurrency 4; source policy 4; writing/history/search 11. This is **36 new
executed tests** beyond the previous 72. Zero-test helper/doc targets are not counted.
The gate reports eight open implementation groups and incomplete requirement coverage;
there is no stale-source, dirty-run, missing-check or untested-input reason.

Development exposed and corrected API mismatches in new tests, a large enum variant
and compressed conditional formatting rejected by Clippy. Review strengthened model
selection/removal generation checks, historical-image protection, library-bound
export plans and hidden graph-point rejection. The integrated restore test initially
used a nonexistent WritingTarget variant; it was corrected and then executed
successfully. The pinned Rust 1.90.0 toolchain/cache needed restoration in this runtime.
No dependency selection or Cargo.lock change was made. The final clean run above
follows these corrections.

Tests do not prove actual recogniser accuracy, Android build success, device latency,
physical power-loss behaviour, OS egress isolation or an independent security audit.
Image fixtures validate byte signatures, not image decoding. No manual/device check
was run or counted as a pass.

## Commit history

| Commit | Change |
| --- | --- |
| `d3c3478` | Approved authority/specification and import provenance |
| `c6ca828` | Transactional organisation and lossless ink |
| `284e1c4` | Recognition proposals and PDF source policy |
| `5d8fc0b` | Encrypted portable subset, converters and recovery tests |
| `84b9489` | Identifier and activation failure handling |
| `ec61644` | Requirement mapping and reproducible backend gate |
| `a222bb3`, `a0fce74`, `0ccd8d1` | Initial 53-test evidence and logging improvements |
| `2e6e8d5` | Shared exercises, durable membership and scoped copies |
| `418fd95`, `3369469` | Exercise/native v2 documentation and 72-test evidence |
| `f0075d3` | Actual capture, object, model, PDF, backup/export code and 36 tests |
| `b077e2c` | Schema-8/native-v3 formats, boundaries and updated traceability |

An evidence-only commit follows; Git records its exact ID. The separate Git bundle
preserves the full commit graph alongside the readable individual files. No remote
has been configured or pushed.

## Remaining 40%: consequential acceptance work

| Area | Still incomplete |
| --- | --- |
| Android backend integration | Kotlin/ABI bindings, input/writer/provider adapters, real authentication, process isolation, diagnostics and OS cloud-backup/device-transfer exclusions |
| Capture host | Asynchronous persistence integration, free-space monitoring, emergency flush, real event cancellation and measured resource/scheduling behaviour |
| Objects/recognition | Region split/merge, component-level acceptance, flow/reflow, layers, overlays/follow-object views, full graph/diagram rendering and actual eligible recogniser adapter |
| Processing lifecycle | Resumable installation, durable converter lifecycle, resource-file cleanup with model leases and hard worker termination |
| PDF workflows | Real provider/parser/rendering and complete host annotation/navigation paths |
| Portability/history | General undo/redo, complete import lineage, chunked/larger-library backup and cross-version whole-backup migration |
| Export | Actual paginated PDF/Unicode/maths layout, static embedded views, cropped image/canvas fidelity and fully visible graph/diagram labels/axes |
| Exercises/coverage | Flat exercise/search projections, caret/selection state, host subscriptions, remaining scale/fuzz/crash cases and complete clause-by-clause acceptance |

The queue is in memory until SQLite acknowledges commit. Cooperative cancellation
cannot terminate an uncooperative native worker. Native v3 is a subset, distinct from
the schema-8 whole-library snapshot; neither claims future feature portability.
Whole-backup restore is currently in-memory and bounded to 64 MiB plaintext. Source
snapshots require explicit independent authority, and live caches are omitted.
Locks are authorisation checks, not working-database encryption. Production H.I.
remains gated. No owner decision currently prevents further core implementation.

All 94 canonical requirements, 103 inherited clauses and 158 historical candidates
remain retained. Related executable tests cover subsets; whole-requirement and whole-
candidate acceptance remain NOT RUN. No numerical SLO was newly approved. Baseline
SHA-256 remains `68612d4dc4d0e69c61f0018fbcf67bf779559bcbb120bcd0f6c3f9333f27b7b3`.
Privacy, stroke-native recognition, replaceable-model design, personal-use/source-
publication distinctions and deferred owner-private manual testing remain controlling.
No handwriting upload, production H.I., proprietary weights or remote inference was added.
