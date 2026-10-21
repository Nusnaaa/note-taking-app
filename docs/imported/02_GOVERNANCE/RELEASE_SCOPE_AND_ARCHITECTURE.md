# Delivery scope and architecture contract

**Read the complete normative specification for detailed behaviour**; this page is a navigation index, not a replacement.

## Release boundaries

| Milestone | Governing interpretation |
|---|---|
| I0 essential foundation | Original ink, natural S Pen inputs, content ownership, offline recognition adapters, editing, durable persistence, privacy, manual/ink-only usability, safe copy/export. Not separately declared first usable release. |
| I1 first usable release | Includes I0 plus functional Module/Project/Notebook/Note/Canvas organisation, root independent material, reference/link/backlink menus, common PDF viewing/linking/annotation as allocated, tables/basic technical objects, chronology, locks, encrypted backup, safe export/import. |
| M1 | Independent Journals and retrospective user-controlled Journal import, mature linked workspaces, advanced links and deeper evidence/resource facilities. |
| M2 | Optional LLM/generative features, optional providers and broader intelligent processing; no dependence for foundational writing. |

**Allowed direct organisational parents**: root → Modules/Projects/Notebooks/Notes/Canvases; Module → nested Module until overall depth 2, Projects/Notebooks/Notes/Canvases; Project → Notebooks/Notes/Canvases; Notebook → Notes/Canvases only. Journals add root/Module/Project locations later (M1), with independent Module journals and project-specific constraints. Notes/Canvases are content, not organisation containers. No nested Notebooks or Projects, no forced default Project/Notebook, Unfiled is a view.

Move preserves ID/relationships/original strokes; Copy generates new identity; Link is a reference to one authoritative object and does not confer new permissions. Incoming/outgoing backlinks exposed via unobtrusive menu; tapping a relationship reveals/highlights its page anchor and simple floating actions. Delete displays dependency warning, Trash/restore maintains associations, permanent deletion exposes clear broken links.

## Architectural placement

- Native Android, **Kotlin + Jetpack Compose** for general H.I.; a dedicated native S Pen ink capture/ink rendering layer as appropriate (test actual pointer/palm behaviour later).
- **Rust-led portable core preferred**, Kotlin Multiplatform comparison remains an engineering option; this approval is *strategy*, not preapproval of every FFI or third-party library. C++ conditional on specific runtime.
- **SQLite-led transactional persistence**, immutable/fingerprinted source ink observations and provenance; editable content separate from immutable input. Document IDs, reference anchors, snapshots, source permissions and exports controlled by app-owned core.
- Shared processing/model lifecycle service; **typed specialised stroke recogniser, converter, and future optional LLM adapters**. Android SDK wrappers can reside on Kotlin side. No engines may own authoritative content or overwrite it directly.
- Model install, version/readiness/capability checks, explicit user selection, optional fallback only with permission, real error/progress states, cancellation and resource limits. Mock implementations during core testing; actual model quality evaluated locally post-build. No covert raster-first path.
- System-protected local storage, user opt-in application locks and export prompts; explicit encrypted full backup by default and OS auto cloud backup exclusion.

## High-level contracts that are not optional

1. Lossless recorded original strokes and source identity survive edit-history expiry, model switch, undo, export and recovery where source retention is required.
2. Typed, genuinely editable prose/maths/code output; preserve invalid math, uncertain structures and code whitespace. A LaTeX string may be one export representation, but cannot by itself prove an interactive editor handles all structures.
3. Recognition/convert proposals must be revision-checked, previewable, correctable and rejected without modifying originals.
4. Source-authorisation, PDF link-vs-copy, snapshot permissions and Q383 revocation survive moves and exports; do not retain unauthorised live-source caches.
5. Essential writing and saved-note access remain usable without installed optional LLM or network; show recognition-readiness separately.
6. Data-loss, save failure, model failure, incomplete conversion and no-op conditions must be reported accurately; never silently misrepresent success.

See `05_SOURCES/verification/Stage5D_94_Requirement_Release_Contract_CROSSWALK.csv`, original 103 clauses and the normative text for per-requirement phase exceptions.
