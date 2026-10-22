# Phase A progress — shared exercise increment

22 October 2026. **Backend acceptance: NOT MET. Production Android H.I.: blocked
by the backend gate.** This is a working partial I0+I1 Rust/SQLite core, not the
complete backend or a runnable Android notebook.

Validated source: `418fd95315f9815672edc0689a13abe497b17809` on `phase-a/backend`.
The working tree was clean when validation began. This report and the captured logs
are committed afterwards as evidence-only changes. The previous 53-test report is
retained in Git at `0ccd8d1`.

## Actual implementation

This increment adds shared master/exercise views over the same editable blocks and
original vector observations. It implements confirmed-heading extraction with a
configurable keyword, stable membership through heading changes, duplicate titles,
revision-safe shared edits, local save state shared within one authoritative Store,
view hiding/restoration, exercise locks, explicit reassignment and durable undo.
Undo survives reopening, preserves intervening content edits and rejects stale or
A-to-B-to-A membership history. No heading operation silently transfers old work.

Locks apply through the master, source ink, history, inference snapshots, search,
anchored links, copying, export and deletion. Reassignment carries protection onto
content. Extracting a locked heading cannot expose its title in a public directory.
Trash rechecks access after review. Appending through an exercise commits its source
observations, master block and membership together, without copying shared ink.

Independent copies have new durable identities, separate later edits, local save
state and mode overrides. Linked exercises follow the master mode. Selected exercise
export excludes adjacent master work. Native subset v2 preserves exercise relations,
membership, heading confirmation/rules and effective modes, and reads v1 files.
Schema migration v2-to-v3 preserves seeded source observations and block history;
failed migration rolls back new tables and the schema version.

Existing organisation, source-preserving ink, block history, literal search,
recognition proposals, typed replaceable local adapter contracts, PDF source policy,
portable encrypted subset recovery, converters and offline help remain implemented.
No actual recogniser is installed.

The validated commit has **92 tracked individual files**, **4,496 physical Rust
implementation lines**, **2,187 Rust test/fixture lines**, and three SQL migrations.
These are measurements of code, not product completion percentages. No ZIP archive
is committed as source.

## Executed validation

The final `python3 tools/validate.py` run was 08:20:25–08:20:41 UTC. Exact commands,
exit codes, output and source hashes are in `latest/validation.json` and adjacent logs.

| Check | Observed result |
| --- | --- |
| Rust formatting | PASS, exit 0 |
| Clippy, all targets, warnings denied | PASS, exit 0 |
| Full workspace tests | **72 passed, 0 failed, 0 ignored**, exit 0 |
| Release build | PASS, exit 0 |
| Traceability consistency | PASS, exit 0 |
| Complete backend acceptance | **NOT MET**, expected exit 2 |

Suite breakdown: SQLite-full unit test 1; converters/help 4; exercise migration 2;
exercises 17; organisation 9; portability/encryption 10; recognition/lifecycle 10;
recovery/concurrency 4; source policy 4; writing/history/search 11. This adds **19
executable tests** to the prior 53-test baseline. Helper and doc-test targets have
zero tests. Deferred manual/device checks were not run or counted as passes.

During development, a test called a nonexistent restore method and was corrected.
An initial offline run lacked a cached dependency; fetching the locked dependencies
resolved it. Clippy rejected two boolean expressions, subsequently simplified.
Review found protected heading-title disclosure, which was fixed and covered by a
new negative regression. The pinned Rust 1.90.0 installation had disappeared from
the transient environment and was restored. One shell invocation lacked Cargo on
PATH; later invocations used its absolute path. Dependency selection and the
lockfile did not change. The clean complete run above follows these corrections.

Passing tests do not demonstrate whole-product completion, device latency, physical
power-loss behaviour, real recogniser accuracy, OS egress isolation or a completed
cryptographic audit. The gate reports only open implementation/coverage groups,
with no stale-source or incomplete-validation reason.

## Commit history

| Commit | Change |
| --- | --- |
| `d3c3478` | Approved baseline, authority and verified import provenance |
| `c6ca828` | Transactional organisation and lossless ink |
| `284e1c4` | Recognition proposals and permission-scoped PDF sources |
| `5d8fc0b` | Encrypted portable subset, converters and recovery tests |
| `84b9489` | Import identifier checks and model activation failure handling |
| `ec61644` | Requirement mapping and reproducible backend gate |
| `a222bb3` | First 53-test evidence snapshot |
| `a0fce74` | Command/exit metadata for silent validation steps |
| `0ccd8d1` | Refreshed 53-test evidence |
| `2e6e8d5` | Shared exercises, durable membership, scoped copies and 19 new tests |
| `418fd95` | Exercise traceability and native subset v2 documentation |

An evidence-only commit follows; `git log` records its exact ID. A separate Git
bundle preserves the commit graph alongside the readable source files. No remote
has been configured or pushed.

## Remaining acceptance work

| Area | Consequential unfinished work |
| --- | --- |
| Exercise integration | Protected placeholders, navigation/search projections, working positions, complete operation history/retirement and host save-state subscriptions |
| Capture | Native event cancellation, bounded persistence queue, capture priority, low-space reserve and tool/view persistence |
| Rich objects | Tables, graphs, geometry, images/assets, groups, canvas cameras/embeds, layout and region/batch acceptance |
| Models | Durable install/profile/update state, resource leases, bounded workers and an eligible actual recogniser integration |
| PDF | Platform provider/rendering, annotations/anchors, reviewed changed-source replacement and permission reacquisition |
| Full portability | Assets/settings/proposals/sources/Trash/history roundtrip, comprehensive encrypted backup, lineage, global undo/redo and PDF/converted-only export |
| Android | Kotlin bindings, ABI/build checks, real authentication, OS cloud-backup/device-transfer exclusions, diagnostics and isolation |
| Coverage | Every applicable clause assertion, remaining interruption/resource/scale/fuzz tests and eventual owner-private device checks |

Transient save failures are shared only inside one Store; separate connections share
durable revisions, not pending status. Writes are synchronous. Host queuing and
retention/retry of unsaved in-memory edits remain open. Native v2 omits reassignment
undo history and refuses source-linked documents: it is not a full backup. Locks
are authorisation checks, not working-database encryption. In-process cancellation
cannot terminate a non-cooperative native worker. Android tooling/integration has
not been validated; no Android build or H.I. success is claimed.

The mapping retains all 94 requirements, 103 inherited clauses and 158 historical
candidate assertions. Whole requirement/candidate acceptance remains unexecuted;
related tests cover subsets. No numerical SLO is newly approved. Baseline SHA-256:
`68612d4dc4d0e69c61f0018fbcf67bf779559bcbb120bcd0f6c3f9333f27b7b3`.

No owner decision blocks further core coding. Continue the open groups in
`docs/verification/backend-gaps.json`. Preserve backend-first sequencing, offline
stroke-native processing, personal-use/portfolio licensing distinctions and deferred
real-model/pen/manual checks. No handwriting upload, production H.I., network
inference, proprietary weight redistribution or dependency change was added.
