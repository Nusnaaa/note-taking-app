# STEM Notebook — project-specific agent instructions (proposed repo AGENTS.md)

The governing source is `docs/spec/approved-v0.5.8.md`, with later owner approval/status decision note. The source Markdown retains a historic `REVIEW CANDIDATE` header that predates explicit owner sign-off; do not treat that title as active state. In conflicts, follow direct owner decisions and current addenda first.

- Develop backend/core first and test it completely before production Android H.I. code. Dev-only CLI and mocks okay. No early manual pen benchmark requirement.
- Kotlin Android and Compose H.I. later; Rust portable core preferred, KMP alternative only with documented decision and approval if strategy changes materially. SQLite-led transactional persistence. No premature model-specific coupling.
- Essential recognition consumes ordered **digital-ink strokes**. Do not upload user handwriting. No mandatory raster pipelines; Pix2Text MFR 1.5 rejected.
- Engine manager shared lifecycle; typed recogniser/converter/future optional LLM adapters. Preserve raw ink independently; revision-safe proposal acceptance. No silent repair of maths or code.
- I0+I1 first usable release: functional Modules/Projects/Notebooks/Notes/Canvases, PDF links, basic backlinks, chronology, safe export/backup. Journals M1; generative LLM functionality M2.
- Personal non-commercial use only; owner will share some original source in portfolio. Verify personal-use rights and source publication separately; do not plan commercialisation, bundle proprietary SDK/model weights, or publish secrets.
- Do not enable automatic OS cloud backup for notebook content; no secret external inference, network egress or model telemetry. Any deliberate model installation/import must be consented and recorded.
- Do not invent quantitative SLOs, passing tests, enabled capabilities, device performance, or licensing permission. Record test commands/results in Git; never conflate synthetic research probes with production tests.
- Do not store ZIP bundles as Git history; commit actual unpacked source and documents with meaningful diffs and provenance.
- If an owner-level question is genuinely blocking, provide a narrow explanation, severity and options. Otherwise make recorded engineering decisions and implement.
