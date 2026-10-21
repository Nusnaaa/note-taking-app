# STEM Notebook — Owner Decision: Provisional Technical Architecture Strategy

**ID:** ST5C-OWN-001  
**Date:** 18 October 2026  
**Status:** **OWNER-APPROVED — PROVISIONAL ARCHITECTURE STRATEGY ONLY**  
**Authority:** Direct owner confirmation following presentation of Stage 5C architecture reconciliation and Technical Architecture & Stack Decision Record v1.0 CANDIDATE.  
**Implementation permission:** **NOT GRANTED**.

## 1. Exact owner statement

> i approve your preferred provisional strategy. proceed under this confirmation

This quotation is an owner-originated decision, not an inferred acceptance from an earlier 'continue'. The decision refers to the preferred provisional strategy explicitly presented in the immediately preceding Stage 5C owner-review work and sourced from `Technical_Architecture_and_Stack_Decision_Record_v1.0_CANDIDATE_2026-10-18(1).md` (§§S10.1, S10.4, S10.5, S10.10) and `Stage5C_Architecture_Owner_Review_PROPOSED_2026-10-18.md` (option A).

## 2. Binding scope of endorsement

The owner provisionally endorses the following **architectural strategy** for remaining specification and feasibility work:

1. **Native Android-first H.I. and platform adapter**, mainly Kotlin; Jetpack Compose is the preferred ordinary H.I. technology, while the authoring surface needs native low-latency inking. Stable Jetpack Ink is the first stylus evaluation candidate, not a tested chosen rendering engine.
2. **Rust-led, portable authoritative domain core** as the preferred initial engineering direction. Kotlin Multiplatform remains an actively evaluated alternative, not a prohibited architecture. Rust without any project-owned C++ may prove the leanest realisation.
3. **C++ is conditional**, used only if an actual specialist engine demonstrates a sufficient benefit to justify maintenance/FFI/build complexity. Transitive C/C++ library dependencies are not thereby prohibited.
4. **SQLite-led transaction store, app-owned canonical source ink and portable interchange** are the favoured storage/data architecture candidates, subject to crash/durability and format testing.
5. **Offline, permission-scoped recognition adapters** are architecturally preferred; Pix2Text MFR1.5 + ONNX Runtime CPU is the first maths *evaluation experiment*, not a certified recogniser, while prose, code, mixed content and correction fidelity retain their own mandatory gates.
6. **Measured architecture decisions:** use equivalent headless contract cases to compare candidates; record resource, maintainability and correctness evidence; do not claim Rust/Kotlin/C++ performance ranking without measurements.

This constitutes approval to **use this design direction in planning and prepare appropriate pre-implementation verification**, not a blanket approval of every ADR-001–184, exact stack/package/version, run permission for feasibility code, or production build order.

## 3. Existing owner decisions preserved

- **ST5A-OWN-001/002:** notebook-first I0+I1, with *usable* Modules and Projects in I1; M1 richer connected workspaces; M2 optional features; future iPadOS much later.
- **ST5A-OWN-003/Q383:** preserve own authored work and historical snapshots with independent continuing authorisation; never expose revoked live-source bytes via ordinary caches/previews/indexes.
- **ST5A-OWN-004:** complete and validate the entire **approved first-release backend** before constructing the production provisional Samsung Notes-inspired H.I.; separate, expressly authorised throwaway feasibility harnesses remain research, not production construction.
- **ST5A-OWN-005:** all 111 existing + eight Stage5B additional application acceptance tests remain **NOT RUN**. Architecture-specific G/BLD/S5/R6/S7/S8 proposals also remain NOT RUN. No application performance SLO is owner-approved by this endorsement.
- No change to the independent canonical Project Development Journal; it is managed in its designated conversation.

## 4. Explicitly *not* approved by this decision

- Specific recogniser, PDF renderer, model provisioning option, Jetpack Ink View-vs-Compose drawing path, JNI-vs-UniFFI, native C++ engine, SQLite PRAGMA, working-database encryption solution or cryptographic algorithm.
- Mandatory model download/packaging policy, platform SDK/API minimum, ABI/binary/version pins, application distribution decision beyond the current personal-use assumption, or fixed numerical acceptance thresholds.
- Final normative specification v0.5.4/v0.5.5; first-release requirement-by-requirement acceptance; successful technical feasibility; initial release readiness; backend or H.I. implementation authorisation.

## 5. Stage transitions / decision precedence

This explicit owner statement **resolves the owner-choice portion** of the Stage 5C `Architecture_Owner_Review` option A and changes the programme's architecture-strategy status from `AWAITING_OWNER_DECISION` to `PROVISIONALLY_OWNER-APPROVED`. It does **not** convert G-02 comparative experiments, G-03 recognition, G-04 S Pen, G-05 persistence, G-06 security, G-08 portability or G-10 backend verification to passing gates. Stage 5C documentary reconciliation is complete; Stage 5D specification/feasibility readiness preparation may proceed under this direction.

**Audit trail:** Preserve the source v1.0 candidate and Stage 5C originals unchanged; record this later owner decision as an addendum. Never backdate historical owner approval into earlier technical stages.
