# Mathematical Handwriting & Digital STEM Notebook — Stage 5D Owner Decision Delta, Q13–Q17

**Recorded:** 19 October 2026. **Authority:** Owner's direct Q13–Q17 answer in the conversation. **Status:** Decisions accepted as described below; outstanding particulars explicitly unresolved. **This is not final full-specification approval or implementation authorisation.**

## Original owner response (verbatim, unmodified)

> 13. i approve. it can work similarly to obsidian's backlinks but obviously adapted for this workflow
> 14. I am okay with the conditional approach
> 15. the way samsung notes handles encryption is pretty good for this as a baseline - ie. normally unlocked notes, added security/encryption i assume for locked/protected notes. I do think that if this ends up being commercialised, the stronger and broaded protection offered by options B and C should be explored for later builds/app version Mx's
> 16. i approve
> 17. i approve. if there is a locked/protected file included in the export, i should get a notification asking if i want that to stay encrypted or not. selecting either option should require me to use biometrics/password/pin or whatever was used to protect the note in order to verify my identity and protect/encrypt the whole export or unclock the locked/protected file to expose it's contents for export depending on which option was selected

## Approved behavioural meaning

- **Q13 — basic links/backlinks:** I1 may show shortcuts referencing one authoritative Note/Canvas/PDF reference, analogous to Obsidian backlinks but adapted to notebook use; removal of shortcut does not delete source and neither backlinks nor shortcuts confer new rights. Advanced linking deferred to M1. Precise scope and granularity of automatic backlink navigation remain unconfirmed; do not silently count Notebook shortcuts as separately approved.
- **Q14 — local recogniser provisioning:** If feasible, include essential offline models with installation; otherwise clearly opt-in to an integrity-verified initial local model setup. Never conceal absent models or use silent network/cloud fallback. Mandatory offline conversion remains non-negotiable.
- **Q15 — ordinary vs locked notes:** Normally unlocked notes with intentional optional content locks, Samsung Notes-like **user-facing baseline**; exact Samsung Notes encryption mechanism is *not* asserted. User expects added protection for explicitly locked content, with encryption strength/working-store design requiring careful tradeoff. Broader protected-payload and full-working-store encryption to be investigated as possible later Mx changes particularly if commercialisation happens; no present commercial scope change.
- **Q16 — backup exposure:** Disable implicit Android OS automatic cloud backups of personal notebook content. Retain explicit user-controlled comprehensive backups, encrypted by default; native OS device-transfer handling still needs separately verified secure policy.
- **Q17 — protected data in exports:** Explicit ordinary native exports may be unencrypted, comprehensive backup encrypted by default. A mixed export with any locked/protected objects triggers an understandable warning and choice: preserve protected status via **encryption of the full exported package**, or explicitly authenticate and permit decrypted inclusion in a plaintext package. **Both branches require successful reauthentication** through the applicable credential/biometric/PIN policy. Do not expose protected content or metadata before authorisation; no silent declassification. Portable encrypted packages additionally need independent access/recovery credentials rather than device-only biometrics/Android Keystore; exact design unresolved.

## Open boundaries for subsequent grilling

- Exact I1 link and backlinks granularity (document, selection/page/heading, Notebook shortcut) and interaction with explicit backlinks display.
- Cryptographic strength for protected notes at rest in I1: in-app access protection is not automatically per-note encryption; keep user-visible claim exact.
- Portable-password/recovery and multi-protected-object auth for encrypted native file export.
- PDF import-as-local-copy vs external reference behaviour, offline permission changes.
- Separate authority for bounded disposable recognition/stylus/core/persistence feasibility experiments; none inferred from these product decisions.

## SDD process evidence entry — SDD-NB-105

- **Method tried:** Multiple substantive owner questions per round, each grounded in a user workflow with recommendation and alternatives, recording approval qualifications instead of binary checkboxes alone.
- **Evidence/results:** Five directly answered questions; Q13 Obsidian-style backlink analogy, Q15 Samsung Notes-style selective locking with conditional future commercial hardening, Q17 new mandatory protected-export reauthentication and whole-package encryption path. Evidence is the verbatim source above; no test results.
- **Failures/limitations:** Generic acceptance options hid a critical export-security branch; a 'Samsung Notes-like' experience cannot establish an underlying cryptographic technical guarantee. Backlink granularity is not sufficiently specified. No benchmarks or code tests were executed.
- **Lesson:** Treat comparative product references as descriptions of desired experiences, not proof of implementation properties; follow approvals with narrow architectural-boundary questions where necessary.
- **Transferability:** The owner-question and exceptions-led method may later be compared with Tano's SDD workflow; untested outside this project.

**Provenance:** Original user message is quoted verbatim; interpretations and unresolved questions are separate editorial additions. Do not retrospectively mark any technical gate or original acceptance case PASS.
