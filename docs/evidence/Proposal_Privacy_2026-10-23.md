# Stored proposal authority before payload parsing — 23 October 2026

Source checkpoint: `69d0d91`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

## Reproduced problem and fix

The Store proposal reader previously deserialised transcription JSON and trusted its
source-block field for permission checks. Synthetic corrupt-row fixtures reproduced
three problems: protected output could nominate a public block through a mismatched
payload, malformed private JSON was parsed before protection, and a mismatched proposal
ID could falsely acknowledge a correction against a nonexistent row. All three actual
repository regressions failed before the fix; original commands/output are retained
in proposal-privacy/regressions-before-fix.log. The baseline was de40478 with only new
fixtures added. These are development corruption probes, not remote exploit evidence.

The reader now checks the app lock before lookup, loads the authoritative SQL source
relation, and authorises that block through current block/exercise/ancestor checks
before fetching/parsing the derived private payload. Proposal and source IDs must match
the SQL key/relation. An authorised caller receives an integrity error for mismatched
identity; an unauthorised caller cannot nominate a public source or reach malformed
protected JSON parsing. The common reader covers review/correct/reject/accept paths.

Original strokes, source fingerprints and revision-safe proposal acceptance remain
unchanged. No payload/schema/receipt format changes or migrations are introduced.
This defends stored relation/key binding, not arbitrary database read/write compromise
or encryption. No recogniser/model was installed or declared production-ready.

## Executed coverage

All 13 recognition tests and strict workspace Clippy passed in development before the
final app-lock assertions. Final assertions also require identical Denied classification
for known/unknown proposal IDs under app lock, authorised integrity rejection and
preserved original source. Complete frozen-source validation passed on matching clean source manifests at
69d0d91: **235 Rust tests**, **131 Android tests**, ten desktop JNI smoke groups,
format, strict Clippy, release, traceability, ARM64/ELF, Android lint, AAR payload and
manifest checks. No tests failed, were skipped or ignored. Commands, hashes, timings
and raw output are archived in proposal-privacy/ and latest/. E01 did not recur in this
run; its cause remains unestablished and its follow-up remains open.

The complete host recognition/proposal lifecycle, real eligible runtime, resource
integration and hard worker termination/scheduling remain open. This preparatory
permission fix does not substitute for those founding-function requirements. See
Recognition_Eligibility_2026-10-23.md for the separate primary-source follow-up.

## Boundaries

No production H.I., handwriting egress, SDK/weights/certificate bundling or network
permission was added. E01 remains an unresolved setup-timeout follow-up; no timeout or
assertion was weakened and no test was disabled. The handover folder remains ignored,
untracked and excluded from commits and pushes.
