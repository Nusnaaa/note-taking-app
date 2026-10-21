# Current authority, approvals and status

**Authoritative update:** 22 October 2026. This addendum records later owner decisions without changing source-file bytes.

## Owner approvals and authority

1. The owner **explicitly approved consolidated v0.5.8** as the normative implementation specification in the conversation following Stage 5F. Preserve its original text and annotate the approval separately. This does **not** mean production tests were run.
2. In a subsequent answer, the owner preferred **actual coding in ChatGPT Work**. They asked for a self-contained Work handover. Preparing this handover is authorised; **actual Phase A code starts only when the owner issues/pastes the Work start prompt**, which explicitly authorises backend implementation. No implementation has been performed merely by preparing these files.
3. First usable application is **I0 + I1** together. I0 alone is not the accepted everyday app. First release includes essential native handwriting, offline stroke-based model-adapter paths, genuinely editable maths/prose/code/numbers, Modules/Projects/Notebooks/Notes/Canvases, PDF study, basic references/backlinks, chronology, safe export/recovery and protection baseline. Detailed inclusions and exceptions remain governed by v0.5.8 and its phase allocations.
4. **Backend first:** build and automatically validate the backend/core **fully** against applicable first-release contracts before starting **production** frontend/Human Interface (H.I.) work. Disposable CLI/test harnesses are permitted and are not production H.I. Backend and H.I. belong to the **same app and repository**. Provisional Samsung Notes-inspired native Android H.I. follows the backend acceptance gate; owner may later replace it with a separately designed H.I.
5. The app is **personal and non-commercial in its present state**. Owner may share **their own source code** in a personal portfolio. No speculative commercial launch planning, distribution preparation or commercial-readiness analysis. A licence allowing personal use may qualify even if commercial use would not; separately verify **rights to publish source and redistribute third-party binaries/models**. Do not commit any proprietary model/SDK/weight without permission.
6. **No upload/disclosure of owner's personal handwriting**, ever as a dependency of implementation or research. Offline-first after local provisioning; no compulsory subscription or cloud provider, and no silent network transmission. Accept stroke-native **ordered vector/digital-ink input**; **no mandatory rasterisation, image/screenshot conversion, or Pix2Text** (explicitly rejected) in recognition baseline. Proprietary SDK internals cannot be claimed stroke-only if undisclosed.
7. Owner approved shared lifecycle/model manager with **specialist recogniser, converter and later optional LLM interfaces**, app-owned original ink and editable representations, immutable input snapshots, revision-checked proposals, safe cancellation/stale-result handling. Model performance remains unverified; **do not claim model switching is universal without compatible adapters**.
8. Owner **accepts the late model quality risk**: use mock/synthetic engines in automated development tests; perform the owner's **manual model, converter and S Pen testing only on a complete runnable app**. Actual end-user conversion adequacy remains a release-acceptance gate, not a gate preventing backend implementation. Auto backend/integration testing remains compulsory.
9. Initial security choice is **Samsung Notes-like baseline**: normal notes unlocked; opt-in app/content lock using Android/device protection, **without a first-release requirement for independent additional database encryption of locked content**. Do not claim a lock supplies at-rest encryption beyond Android protection. Android implicit automatic cloud backup of notebook contents disabled. Full comprehensive backups encrypted by default.
10. If export includes protected items, notify; **both encrypted and deliberate unencrypted export paths require authentication** for all protected scopes. Encrypted portable exports must have a portable credential/password (not just device-only biometrics), and a receiving device can establish its own protection after successful import. Avoid weak PIN-only offline encryption; apply strong key derivation and adequate attack resistance while retaining a usable H.I.
11. Explicit review/correct/accept/reject **before recognition replaces/inserts editable content** is approved in Q084 and reasserted by Stage5F; not an open choice. No silent math solving, correcting or code rewriting.
12. Journals and Journal import are **M1**, advanced linking and mature workspaces likewise later; optional LLM functionality is **M2**. Model-manager seams may be designed now without shipping future features. Full Module/root parent matrix approved Q23; no nested Notebooks, no nested Projects; maximum Module depth = 2.
13. Delete to Trash warns of dependent references; restore reconnects original links; permanent delete leaves broken referents; root 'Unfiled' is a virtual view of genuinely standalone work. Basic chronological browsing in I1. Link-first PDFs with optional independently authorised managed copy; Q383 source permission/revocation restrictions remain binding.
14. **SDD scope:** maintain notebook-specific requirements, decisions, evidence, limitations, reproducible tests. **Do not create a generic reusable SDD methodology** within this project; owner is developing that separately.

## Status and interpretation rules

- v0.5.8 first lines say `REVIEW CANDIDATE` because they were authored *before* owner's approval. The **content is now approved**; the historical header remains evidence of its original state, not a new blocker.
- No real model has been approved as the shipping backend; the research papers/SDKs are **candidates**, not verified for device performance or licence compatibility.
- Prior candidate technology record includes obsolete Pix2Text/image-crop suggestions; its architecture strategy is directionally approved, **but each obsolete ADR or specific SDK/benchmark is not**.
- The inherited prebuild manual recognition/palm experiment gates in old archived text were **rephased by later owner decision**; do not impose them before backend coding.
- The 16 preliminary numerical performance SLOs are **unapproved** and not observed. Do not turn them into binding arbitrary pass thresholds.
- No production app tests are reported run; past synthetic/research experiments cannot count as passing the app's backend suite.
- 131 existing proposed atomic/alias tests and 27 supplementary suggestions (158 candidate rows) require review, deduplication and implementation. 2 of 94 requirements have test IDs `TBD` as of Stage5F: `NB-SUP-001`, `NB-SUP-002`. The 103 legacy clauses were retained, **not fully proven semantically mapped**.

## Precedence on conflicts

A. Explicit direct owner decisions, including 22 October approval and Work preference.
B. Current-state §§0.8.0–0.8.10 of approved v0.5.8 (not historical titles/statuses).
C. Prior owner decisions / inherited clauses reproduced in v0.5.8 where compatible.
D. Stage5F audit and latest phase/verification records, as evidence/engineering planning rather than novel product decisions.
E. Provisionally approved Kotlin/Rust/SQLite native Android architecture strategy; selected libraries require engineering verification.
F. External research package: supporting facts, suggestions and *unapproved* example schemas only.

If a design tradeoff affects security, data permanence, essential first-release scope or a binding owner decision, stop **that change** and present a focused choice. Keep unrelated safe work moving. Do not ask the owner to approve an exhaustive list of routine implementation choices.
