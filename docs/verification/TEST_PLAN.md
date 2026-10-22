# Current I0+I1 implementation and verification map

The three generated CSVs preserve all **94 canonical requirements**, **103 inherited
clauses** and **158 historical candidate rows**. The last number is not an executed
test count. `tools/traceability.py` checks row retention, explicit current-phase
mapping, source-clause presence and real executable references. It does not certify
semantic completion. `validation.json` records observed execution separately.

Requirements map to code paths, related test functions, phase and remaining work.
The current implementation covers subsets; whole requirements and whole historical
acceptance candidates remain `NOT RUN` until every applicable assertion is exercised.
No historical source CSV has been edited to manufacture current results.

## Deduplication decisions

Use one fixture family for overlapping setup and retain each source outcome. Do not
multiply the number of distinct tests merely because several IDs cite that fixture.

| Shared fixture / outcome | Retained overlapping source IDs | Execution boundary |
| --- | --- | --- |
| Parent matrix, depth, invalid moves | CF-02, ST5B-I1-02, ST5E-T-14; broader TC-07-I/SV-27-I/S4C-019-I | Existing organisation tests cover core parent/depth transactions; broader cases still retain their other assertions |
| Unfiled and stable identity | CF-01, ST5B-I1-04, ST5E-T-15; S4C-003 | Share organisation setup; preserve distinct timestamps, source/history and reference assertions |
| Source survives history expiry | CF-22, SV-10, S4C-004 | Share source/history fixtures; presentation toggles and full source-reference checkpoints are separate retained outcomes |
| Missing/failed model readiness | CF-24, SV-09, SV-26, S4C-024, ST5E-T-10 | Share manager setup; actual installed-model/offline host path is still pending |
| Source revocation | ST5B-I1-08; CF-10/SV-08/SV-25/S4C-025 later-context variants | I1 policy test does not certify M1 catalogue/overlay propagation |
| Protected portable export | ST5E-T-21, ST5B-I1-07, ST5E-T-12 | Shared auth/export fixtures; do not drop recognition access or other-scope assertions |
| Native recovery | ST5B-I1-05, ST5E-T-22, ST5E-T-20 | Existing subset roundtrip cannot satisfy comprehensive backup or Android OS-backup exclusion |
| Shared exercises and membership | CF-25, CF-28, S4C-002, S4C-026 | Exercise fixtures cover shared revisions/status, heading stability, explicit reassignment/undo, independent copies and v2 subset migration; general history, host navigation and positions remain open |
| Post-build quality and pen use | CF-29/CF-30, ST5E-T-26/ST5E-T-27 | Separate deferred owner-private checks; synthetic adapters never substitute |

The candidate ledger records all other fixture families and all unabridged expected
outcomes. Mixed-phase aliases remain separate; generic repeated wording such as
“run after implementation” is not grounds to merge different M1 features.

## Search and Help gaps closed in the test mapping

`NB-SUP-001` now points explicitly to
`writing::search_matches_literal_accepted_content_never_raw_or_locked_data`.
It covers literal accepted content, titles, raw-ink exclusion and access checks.
Diagram labels/headings, new entity paths and scale remain unfinished.

Exercise tests additionally cover title non-disclosure from protected headings,
protection through master/history/inference/links/export, and failed shared saves.
V2 migration tests seed actual v2 source observations and history before upgrading;
an empty-schema upgrade alone is not treated as preservation evidence.

`NB-SUP-002` now points explicitly to
`converters::offline_help_covers_recovery_and_unsearchable_ink_without_remote_dependency`.
It covers bundled recovery topics with no remote lookup. Full contextual integration
and usability are unfinished. These replace historical `TBD` test references in the
current map, not in the approved text, and do not declare whole requirements passed.

The inherited `SUP-002` text concerns unrecognised-ink search, although its editorial
arrow names `NB-SUP-002`; the current semantic link is `NB-SUP-001`. `SUP-003`
concerns Help although its old arrow names model management; it now maps to
`NB-SUP-002`. `SV-16` has an M2 label but I1 canvas-embed camera/gesture semantics;
the current plan retains its I1 backend invariant and post-gate host action.

## Required next validation

Complete and test every open group in `backend-gaps.json`, then close applicable
assertions individually with executed evidence. Add bounded malformed-format/fuzz
coverage, remaining interruption boundaries, scheduler/resource contention,
model-install rollback, graph/source lock paths, full backup migration/restore,
and automated Kotlin/Android ABI/security configuration checks. Keep device quality
and H.I. tests separate. None of the 16 proposed numerical SLOs is approved here.

The existing process-exit test ends without Rust destructors; it is stronger than a
normal close/reopen test but is not a physical power-loss test. SQLite-full testing
uses SQLite's actual page limit to produce a real SQLITE_FULL transaction failure;
it does not establish emergency-reserve behaviour. Cargo `--offline` is dependency
offline mode, not an OS network-egress audit of a future native SDK.
