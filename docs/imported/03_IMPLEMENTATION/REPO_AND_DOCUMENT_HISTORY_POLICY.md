# Git, repository and documentary provenance requirements

## Repository setup

Use a **single application repository** unless evidence establishes a real separation requirement. Local Git is acceptable initially; **do not publish or push** to any GitHub remote without a specifically selected and authorised destination. A private repository is sensible while dependencies, personal data protections and licence handling are assessed. Do not imply a remote has been connected or created unless confirmed.

Proposed top-level directories (adapt based on tooling and existing repository): `core/`, `android/`, `adapters/`, `tests/`, `tools/`, `docs/spec/`, `docs/decisions/`, `docs/evidence/`, `docs/implementation/`, `fixtures/synthetic/`, `.github/` if hosted CI is available.

**Commit text/code files separately** as useful Git diffs, with small coherent commits and reproducible build/test instructions. Do not commit archives as stand-ins for source, no repeated handover ZIPs, generated APKs, build caches, virtual environments, entire research datasets, private user notes, real pen recordings, SDK secrets, vendor weights or restricted proprietary code. Do not commit tests only after they pass; write and commit tests as part of normal feature development, accurately recording failing/incomplete status where relevant.

## Reconstructing historical documents without falsifying history

Project documents predate initial Git setup. Import chosen authoritative Markdown/CSV/JSON files **as ordinary readable content** using a truthful migration/provenance commit, e.g. `docs: import pre-repository project specification and decision records`. Preserve embedded original creation dates, revision identifiers and source SHA256, but do **not** rewrite Git author/commit dates to pretend previous Git commits existed.

Preserve approved v0.5.8 **byte-for-byte** as imported baseline with an accompanying approval/status addendum. When new specifications supersede it, create explicit versioned revisions and document change rationale rather than silent in-place amendments to archival source. Put later sign-offs in separate decision files.

## Evidence and routine developer notes

- Every test document states `NOT RUN`, `PASS`, `FAIL`, `SKIPPED`, or `BLOCKED` based on actual execution and provides the command, environment, test IDs, logs/artifact path and commit SHA.
- Maintained project-specific SDD evidence (context/method/result/limitations/lesson/transferability when relevant) continues; **no generic reusable SDD methodology** to be developed here.
- ADRs are specific to actual technical decisions. Clearly distinguish owner-approved strategy, engineering-selected detail, vendor claims and measured implementation results.
- Before a new protected data/third-party resource is committed, ensure it is authorised for public portfolio sharing; personal use rights do not necessarily grant distribution rights.

## Useful initial repository files

`README.md`, `AGENTS.md` (project-specific coding constraints), `.gitignore`, `LICENSE` only if consciously selected for the owner's source, `docs/spec/approved-v0.5.8.md`, `docs/decisions/owner-approval.md`, `docs/implementation/status.md`, plus actual project source/build/test files. **Do not infer an open-source licence for the owner's code** just because it may appear in a portfolio; ask only at the point public licensing becomes consequential.
