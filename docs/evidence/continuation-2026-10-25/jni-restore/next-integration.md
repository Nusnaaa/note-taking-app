Next implementation: Android recovered-copy controller (not an enabled capability).

- Add a NotebookRestores controller owned by NotebookBackend. Use one bounded
  optional worker and retain the library lease through pre-admission input open,
  native decode/review/publication, post-native acknowledgement and real cleanup.
  Add invalidation on access/policy lifecycle changes and await actual retirement
  in closeFullyAsync before releasing LibraryOwnership.
- Deliberate picker input must own its read grant/descriptor and cancellation
  signal. Never turn provider callbacks or user-supplied paths into ordinary JSON
  capabilities. Cooperative cancellation must not claim hard termination or free
  admission/ownership while a provider callback still runs.
- Use private mode-0700 staging, engineering byte/quota bounds and clear orphan
  review. A recovered destination is a new independent root, cannot overwrite the
  live database, and is not activated automatically. Preserve protected content,
  original ink and imported-source regrant requirements.
- Separate stage review, fresh proof/confirmation and Published/Uncertain outcomes.
  Current capture/private recovery drafts must remain untouched. Stage disposal,
  relock, close races and malformed/provider/resource failures need synthetic
  JNI/controller tests; actual device/provider evidence remains separate.
- Run complete stable-source Rust and Android validation after integration. The
  25 October development full-suite run was not a final exact-source acceptance
  record and must not be promoted to one.
