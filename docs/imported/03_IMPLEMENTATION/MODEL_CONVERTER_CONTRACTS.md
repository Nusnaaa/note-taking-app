# Replaceable recognisers, converters and future LLMs — engineering constraints

**Status:** binding product invariants plus *proposed* architectural interfaces. The exact JSON schema, FFI type and selected runtime are engineering work, not independently owner-approved.

## Three fundamentally different adapter families

- **Ink recognition**: typed stroke sequence/snapshot + explicit recognition task and model settings → alternative editable proposals, confidence where actually available, diagnostics and mapping to source strokes. Foundational I0/I1 functionality.
- **Conversion**: typed, accepted editable content + format plan → target representation (LaTeX, MathML, Markdown, code, native, PDF etc.) and fidelity report. Foundational where allocated; conversion does not silently reinterpret mathematics.
- **Generation / LLMs**: optional later M2. Keep extensibility seams only; do not require generative models for writing, recognition or maths conversion.

A common manager may handle discovery, metadata, readiness, install/remove, version pinning (where truly supported), task routing, cancellation, memory scheduling and health. **Do not insist on a fake universal `infer(anything)` contract.** SDK-owned model versions and download APIs may not support arbitrary byte pinning; model compatibility is per capability and adapter.

## Example engineering *sketch*, not normative schema

```text
InkCapture -> ImmutableStrokeSnapshot(source_id, revision, selection, channels)
             -> TaskProfile(math | prose | numeric | code | mixed)
             -> InkRecognizerAdapter.recognise(snapshot, profile, options)
             -> ProposedEditableResult(candidates, mapping, diagnostics, source_revision)
Core.acceptProposal(proposalId, expectedCurrentRevision) -> atomic commit | conflict

AcceptedEditableDocument -> ConverterAdapter.convert(snapshot, target_format, policy)
                         -> artifact + fidelity_report + unsupported_content
```

- Engine never receives writable authoritative database handles. Core owns revision and privacy policy.
- Original observations are separate from transformed/normalised inputs, renderer points, output tokens and edit proposals.
- Code mode preserves exact case, whitespace, newline, punctuation and even syntactically invalid expressions; maths mode preserves ambiguous and intentionally incorrect expressions.
- Model install decisions are explicit, offline after provisioning; no secret telemetry/remote fallbacks; verify every provider's actual lifecycle and licence for personal use.
- Work may select a mock/test recogniser and converters first to validate contracts. It must not describe mocks as user-ready mathematical recognition. At completed-build stage, allow owner to install compatible actual models and compare them privately; model performance may block final acceptance.
- If a model is fundamentally raster-only, label incompatible with the approved stroke-native founding path; never silently insert an ink-to-image stage. `Pix2Text MFR 1.5` is expressly rejected.

See `05_SOURCES/research_advisory/03_Model_and_Converter_Architecture.md` for possible schemas and integration risks; research proposals do **not** override the current specification or confer permission to use vendor binaries.
