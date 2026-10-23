# Recognition integration follow-up — 23 October 2026

A4 remains OPEN. No new recogniser, SDK, model, certificate or resource was installed
or selected. The core Manager/Store processing contracts exist, but Host currently
does not expose a real bounded recognition/proposal lifecycle. A compiled trusted
adapter/job integration is still required, independent of final vendor/model choice.
Synthetic recognisers cannot count as the founding recognition function.

Primary-source checks during the capture-discard validation:

- [Google Research MathWriting README](https://github.com/google-research/google-research/blob/master/mathwriting/README.md)
  documents digital-ink data, reading/evaluation examples, CC BY-NC-SA 4.0 data and
  Apache 2.0 example code. This inspected page does not establish a ready Android
  recogniser or a separately licensed checkpoint; no dataset was downloaded.
- [MyScript official Android examples](https://github.com/MyScript/interactive-ink-examples-android)
  require an application certificate and distinguish the integration examples from
  the runtime. Example-source availability alone is not runtime permission.
- [MyScript published on-device terms](https://github.com/MyScript/interactive-ink-licenses-android/blob/master/LICENSE.txt)
  (repository search result identifies V8, effective August 2025) distinguish
  development/testing permissions, restricted internal/evaluation/demonstration
  runtime licences and distribution rights, and describe online certificate-server
  activation. [Official pricing](https://developer.myscript.com/pricing/) likewise
  separates testing/development licences from deployment. This is evidence to review,
  not an automated legal approval or a claim that personal use is prohibited.

No evidence here verifies the required no-mandatory-raster internals, current Android
resource/worker behavior, offline readiness or source-publication rights for an actual
chosen adapter/model. Current no-INTERNET/privacy boundaries remain unchanged. Model
installation/activation would require the specified explicit consent and recorded
provenance; no credentials or proprietary artefacts belong in Git.

Next implementation work can add the missing bounded, permission-scoped trusted
recognition job/proposal path without inventing installed/readiness state. The full
backend gate must remain closed until actual eligible runtime integration and all
other required work are verified. No owner handwriting samples are needed at this
backend stage; final private quality checks remain allocated to the runnable build.
