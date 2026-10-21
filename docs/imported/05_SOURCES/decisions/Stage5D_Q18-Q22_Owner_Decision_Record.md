# STEM Notebook — Stage 5D Grill-me Owner Decision Delta, Q18–Q22

**Date recorded:** 2026-10-19. **Authority:** owner direct conversation reply, quoted verbatim. **Status:** product/process decisions approved with qualifications; not implementation authorisation. **Source baseline:** Stage 5D v0.5.6 candidate and Q13–Q17 owner-delta record. This is a non-destructive addendum, not a rewrite of historical records.

## Exact owner wording (verbatim)

<verbatim owner-response>
18. I approve, but we would need to go into quite a bit of detailed work to ensure that this feature doesn't become too complex to actually use, since this workflow is not liek obsidian's primarily text and typed based workflow. for the first release, displaying incoming and outgoing backlinks should be in a menu item and when tapped, highlight/display the actual relationships on the page with the links for now, a simply drop floating menu after tapping on a links/backlinks menu list entry should provide the functionality intedned for release 1
19. option A is fine for the first release
20. yes. the laptop can simply use a different authentication credential after successful import. no need to overcomplicate it. the export bears more of the weight. it might be good to force the exporting user to set a simple pin or password which will need to be entered on the device being exported to authenticiate that the files are allowed there before switching to that devices authentication credentials and granting full control to expose or keep that file protected
21. linking should be the default, but yes. i spprove supporting both as well as your wider recommendation
22. fine. i approve
</verbatim owner-response>

## Approved interpretation and acceptance implications

- **Q18 I1 links/backlinks:** user endorses adapted Obsidian-like explicit links and backlinks (including selected handwritten regions and Notebook-level shortcuts as previously proposed) with **deliberately lightweight H.I.**. Incoming/outgoing links are accessible by a menu item; selecting an entry highlights or displays the actual on-page linked relationship/target, with a simple contextual floating/drop menu for first-release actions. **Do not require full-page relationship graph or complex visual edge overlays in I1.** Validate ease of use with natural ink; menu placement, highlight/lifetime and action details remain Human Interface design candidates.
- **Q19 I1 protection:** owner chooses Option A — Android sandbox/device baseline and application/content locks, not an additional first-release application-level encrypted working database or separately encrypted locked-note payload. Already approved protection of restricted content in app projections/search/previews/export still applies. This is not equivalent to claiming independent at-rest ciphertext security; revisit B/C in later Mx, especially if release/commercial context changes.
- **Q20 portable handoff:** user approves successful authorisation on source device, export-specific portable password/PIN credential to decrypt an encrypted entire package at destination, then destination may establish its own credential(s) for local protection. Retain Q17 authentication for either protected-export branch and explicit choice of encrypted versus deliberately revealed ordinary export. **Security engineering caveat:** weak PIN alone may not withstand unlimited offline guesses, so adequate KDF/password entropy, rate limiting where enforceable and honest portability/recovery guarantees require technical assessment. No device-bound biometric/Keystore credential as sole means to open portable package.
- **Q21 I1 PDF source acquisition:** both external provider link and independently authorised in-app managed local source copy available; **link is default**. No silent duplicate, provider revocation bypass or continued access to revoked-source caches. Q383 independent snapshot provenance applies to any independently authorised retained copy.
- **Q22 research authority:** owner specifically approved four **narrow disposable non-production feasibility experiments** (EXP-01 offline recognition, EXP-02 real S Pen/palm, EXP-03 Rust-vs-KMP contract slice, EXP-04 persistence/recovery/security spike). Use synthetic or expressly cleared non-confidential data, prevent automatic cloud release, record source and measured evidence; physical Samsung device results still impossible until suitable device access/data. User's confirmation is NOT production backend/H.I. implementation authorisation, NOT model download/network/account authorisation beyond the bounds of each experiment, and is not an experiment result.

## Remaining owner/design frontier

- Final exhaustive allowed `(child,parent)` matrix and I1 creation/navigation behaviours; no Notebook nesting.
- Detailed but approachable links/backlinks H.I. design and on-page highlighting, suitable for handwriting; can be refined by usability evidence instead of another vague blanket approval.
- Determine secure, user-comprehensible portable package credential construction and authentication transition through actual threat/recovery tests; do not equate short PIN with strong portable archive encryption.
- Hardware evidence gates, calibration and whole specification sign-off remain open. All proposed application test cases **NOT RUN**.

## SDD evidence — SDD-NB-106

- **Method:** A multi-question evidence-aware owner grill on tangible first-release interactions and threat boundaries; preserve raw user corrections alongside derived decisions.
- **Evidence/results:** Q18–Q22 approvals with menu-based backlink H.I., first-release Option-A locks, portable import credential handoff, default linked PDFs, and explicit permission for bounded test harnesses. This entry documents decisions only; no experiment execution.
- **Failure/limitation:** Earlier schematic backlink concept risked excess complexity for pen-based app; user expressly limited initial interaction. A portable 'simple PIN' can be misleading if treated as strong encryption; choose verified key derivation/adequate secret and honest claims. Source application readiness still untested.
- **Lesson:** User-approved interactions require human-scale affordances rather than general-purpose graph presentation. Keep producer authentication, encrypted transport and recipient local authentication as distinct stages.
- **Transferability:** Useful hypothesis for Tano SDD (owner corrections narrow scope and prevent feature bloat); not yet independently tested across projects.

**No new test PASS and no full production authorisation recorded.**
