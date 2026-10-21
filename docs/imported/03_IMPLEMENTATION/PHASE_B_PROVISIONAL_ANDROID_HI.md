# Phase B — usable, provisional native Android Human Interface

**Do not begin until Phase A backend acceptance report demonstrates full applicable backend validation.** This is a subsequent phase within the **same project, Git repository and Work implementation programme**.

1. Implement a genuinely useful Samsung Notes-inspired (not Samsung Notes copy or proprietary code reuse) native tablet H.I.: pen-centric ink canvas, selection and editing, palm/hover/touch routing, save/recovery state and navigation.
2. Complete first-release Module/Project/Notebook/Note/Canvas creation and containment controls (including Notes/Canvases directly in Projects/Notebooks; no nested Notebooks). Root Unfiled; simple chronology. Show disabled/invalid create/move options and reasons.
3. Show explicit, unobtrusive link/backlink menu; tapping entry reveals/highlights location and a small floating action menu. No complex graph requirement. Support dependency-aware deletion, Trash, restore and broken references.
4. Offer split-view PDF study, link-by-default PDF import and optional authorised managed copy, missing file/revocation/move/relink states, annotations as allocated. No unapproved cached sources.
5. Provide conditional model setup/download/import, recognition-ready indicators, task-specific model profiles, per-operation selection and switching, clear status/errors, result preview/correct/accept/reject. If real models are not yet available, clearly show unsupported/not-ready rather than faking recognition.
6. Implement genuinely editable structured maths/prose/code interaction, correct clipboard/export, basic tables/graphs/images where phase-approved and required. Do not advertise editing a raw LaTeX string as equivalent to full structured editing if not true.
7. Provide app/Note locks, protected-export prompts, backup/recovery and per-file/private/offline assurances; do not describe H.I. lock as independent data encryption unless implemented and verified.
8. Produce installable build (APK or appropriate Android testing artifact); make it easy for the owner to evaluate models, converters and stylus privately on Galaxy Tab S11 Ultra after app ready. The local model roster is vendor/licence/runtime-dependent; no guarantee that all research examples are installable.

Keep automated Android integration tests and project source tests in Git. A backend gate cannot be passed by merely promising to test everything on the tablet later. Device-specific S Pen palm rejection, latency, battery, recognition accuracy/quality and ergonomics are explicitly queued for manual owner-local acceptance **after this phase**. Package the completed-build manual checklist with each recorded observation marked NOT RUN until actually performed.
