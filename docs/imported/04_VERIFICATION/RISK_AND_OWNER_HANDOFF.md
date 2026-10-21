# Risk register digest and escalation plan

Current risk acceptance: owner explicitly accepts the possibility of discovering poor or unavailable production-grade stroke-native recognition **only after the runnable app exists**. Do not block backend work on performing manual model benchmarks now. The following risks remain explicit:

| Risk | Severity/owner policy | Backend mitigation | When actual verification occurs |
|---|---|---|---|
| Stroke-native offline maths/code recognition unsuitable | **Critical product acceptance risk accepted for deferral** | Typed capability and revision-safe adapters, lossless ink, app-owned editable content, truthful not-ready statuses | On completed native Android build, owner-private manual candidate comparison |
| Incompatible model/runtime/vendor licence | High integration risk | Per-type typed adapters, capability manifest, explicit installation checks, exact provider permissions; synthetic contract tests | During adapter integration; real device performance afterwards |
| Writing under memory/thermal load | High usability risk | Prioritise capture, bound queues, cancellation, no authoritative model writes | Structural concurrency tests during backend; device thermal/perf later |
| Palm rejection / tablet pen handling | High post-build H.I. risk | Input schema, cancellation rollback/ownership | Native H.I. completed build and local S Pen manual check |
| Lost original strokes / corrupted database | **Non-deferrable backend safety** | Transactional store, migrations, interruption simulation, checksums/identity, recovery tests | Backend gate; additional device validation later |
| Protected content unintentionally exported | **Non-deferrable backend safety** | Credential checks, package encryption, explicit dangerous-export path | Backend gate and later H.I. test |
| Reference permissions and revoked PDF caches | **Non-deferrable backend policy** | Authorised source state, explicit local copies, missing/revoked status, no stale unauthorised caches | Backend gate and completed-build manual smoke test |
| SLO threshold or test status invented | Governance risk | `NOT RUN` default, measurement logs, no fabricated pass numbers | Every delivery checkpoint |
| Public portfolio accidentally includes SDK/model/proprietary assets | Licence/privacy risk | Source audit, `.gitignore`, separate personal execution vs publish rights | Before publishing or committing any restricted asset |
| Archives and historical docs swamp repo | Maintenance risk | Import selected plain-text source and citations only; record original dates/hash, reconstruct Git history honestly | Initial repo setup and future documentation commits |

## Owner interruptions

Escalate when a design fundamentally violates direct handwriting, privacy, preservation, release allocation, storage safety, or there is no legally valid personal-use path for a necessary engine. Provide impact/severity and a recommended minimally disruptive path. Engineering details that preserve contracts can be decided and logged without repeatedly grilling the owner.

## Stage handoffs

**Current**: normative baseline approved; implementation handover being supplied. **Next in Work**: owner-issued production Phase A start prompt, local Git and backend build. **Later in same project**: backend acceptance report → production Android H.I. → installable device build → owner-local model/pen/converter tests. No phase should be represented as done without code, commands, passing results and evidence.
