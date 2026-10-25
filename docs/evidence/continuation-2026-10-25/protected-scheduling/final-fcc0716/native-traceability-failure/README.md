# Exact-source native run with stale generated map

`python3 tools/validate.py` against fcc0716 terminated with exit 1. Formatting, Clippy, all 335 native workspace tests (zero failed/ignored) and release build passed. The final `python3 tools/traceability.py --check` failed because generated `requirements-current.csv` had not been regenerated after adding tests. This is a failed full validator, not a passing complete run. All 270 tracked non-evidence source hashes matched the root checkout at archive time; the Android archive for the same source separately passed all 324 tests and all 28 normal/R8 JNI fixture groups.

`isolated-workspace-335.log` separately records the earlier `cargo test --workspace --locked --offline` command's exit-0 335-test development run in `/tmp/stem-protected-backup-work`; that checkout was based on 5d2efb2 with the implemented source byte-matched to fcc0716. It is not a clean-commit run and does not replace the failed final traceability check.

Next action: regenerate the traceability maps, review the generated diff, run their check and retain the correction with truthful verification scope. Backend acceptance and production Android H.I. remain false; all eight backend gaps are open.
