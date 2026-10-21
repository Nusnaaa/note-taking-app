# STEM Notebook — ChatGPT Work implementation handover

**Packaged:** 22 October 2026. **Owner approval:** Consolidated Specification v0.5.8 approved by owner in this conversation after the 21 October review. **Implementation state:** no production source code, passing production backend, validated Android H.I., or recognised personal handwriting is represented as existing. This package is a **handover**, not an assertion that implementation has started.

## How to begin

1. Open **a new ChatGPT Work conversation** (Work mode, not an ordinary chat).
2. Attach **this ZIP** and paste `01_START/WORK_START_PROMPT.md` (or the prompt in the accompanying response). Work should extract the ZIP into its workspace, read `02_GOVERNANCE/CURRENT_AUTHORITY_AND_PRECEDENCE.md` first, then follow the defined read order.
3. Pasting the start prompt intentionally authorises Work to **begin production Phase A (backend only)**. It does not authorise skipping backend acceptance or immediately implementing the Android H.I.
4. If a GitHub repository is accessible through the connected app, Work may inspect/use the approved repository after checking ownership and status. Otherwise initialise local Git and **do not invent a remote, publish code, or push to an unconfirmed repository**. Work may ask only for a truly blocking repository/access choice.
5. Ask Work for concise evidence-led progress reports: completed work, tests actually executed, outstanding risks, and actual commit hashes. No claims of autonomous background execution unless the environment provides it.

## Trust and precedence

`05_SOURCES/normative/STEM_Notebook_Consolidated_Specification_v0.5.8.md` is the exact **approved baseline content**, despite the file's preserved historical title/header reading `REVIEW CANDIDATE`. The **later owner approval** is recorded in `02_GOVERNANCE/CURRENT_AUTHORITY_AND_PRECEDENCE.md`. Do not rewrite the original specification header in place or cite its old status as present-day refusal to implement.

Highest: explicit owner decisions through 22 October 2026, including the approval in this chat; then current-state v0.5.8 §§0.8.0–0.8.10; then its older preserved requirements/accepted owner decisions where not superseded; then phase and traceability records; then architecture strategy (approved direction); then external research (advisory, not owner approvals). If conflict remains, document it and ask a targeted owner question **only** when material to safe implementation.

## Document map

- `01_START/WORK_START_PROMPT.md` — send this to Work.
- `02_GOVERNANCE/CURRENT_AUTHORITY_AND_PRECEDENCE.md` — binding approvals, statuses, superseded material.
- `02_GOVERNANCE/RELEASE_SCOPE_AND_ARCHITECTURE.md` — first release and deferred capabilities.
- `03_IMPLEMENTATION/PHASE_A_BACKEND_PLAN_AND_GATE.md` — stages, expectations, nonnegotiable backend validation gate.
- `03_IMPLEMENTATION/MODEL_CONVERTER_CONTRACTS.md` — engineering contract requirements; research recommendations remain advisory.
- `03_IMPLEMENTATION/PHASE_B_PROVISIONAL_ANDROID_HI.md` — future H.I. scope and gate.
- `03_IMPLEMENTATION/PROJECT_AGENT_GUIDELINES_AGENTS.md` — proposed repo-root `AGENTS.md` for Work to adapt.
- `03_IMPLEMENTATION/REPO_AND_DOCUMENT_HISTORY_POLICY.md` — Git, security, reproducibility, test and document provenance.
- `04_VERIFICATION/ACCEPTANCE_AND_TRACEABILITY_WORKPLAN.md` — planned evidence, unresolved links, dedup process.
- `04_VERIFICATION/RISK_AND_OWNER_HANDOFF.md` — critical risks and how to surface them.
- `05_SOURCES/` — unchanged documentary inputs (avoid committing duplicated historical bundles/ZIPs to source repo).

## Critical distinction

The ZIP is only **portable transfer packaging**. In the eventual project Git repository commit the relevant extracted Markdown/CSV/JSON/source/test files as **individual readable files**; **do not commit handover ZIPs**, bulk backups, proprietary binaries/model weights, generated caches, virtual environments, or unpublished handwriting to Git. Preserve source provenance and the original creation dates; don't create a fake Git history asserting old documents were originally committed on their creation date.

## User's involvement

The owner does **not** need to install models, upload handwriting, or conduct real S Pen/model/comparison tests to begin the backend. Those manual tests take place **locally on their tablet after a complete runnable Android build**. Automated contract, data integrity, import/export, security, migrations and model-swap tests run during implementation and before production H.I. begins.
