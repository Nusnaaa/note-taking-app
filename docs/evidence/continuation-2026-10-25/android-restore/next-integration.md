Next implementation targets (design notes, not enabled/tested capability):

1. Add explicit unfinished-stage review/cleanup. The current controller reports and
   preserves any nonempty orphan staging but cannot yet review/discard it. Persist
   a purpose/library/operation marker before decode; bind reviews to exact bounded
   metadata/digests and current private-root ownership. Do not follow symlinks or
   inspect a stage still owned by an admitted native/controller job. Do not treat
   an arbitrary filename as permission to delete. Unknown bytes need preservation
   or explicit quarantine rather than automatic cache sweeping. Keep cleanup on
   optional workers and acknowledge partial/uncertain effects truthfully.
2. Add hard native staging allocation controls and bounded semantic validation.
   Current ciphertext/plaintext/free-space limits do not cap SQLite amplification,
   WAL/journals/temp files or whole-graph validation memory. SQLite max_page_count
   alone is a main-database bound, not a whole-staging disk quota. Any scratch-only
   journal/pager strategy must preserve live-library journaling and make interrupted
   private stages unpublishable until complete revalidation. Publication temp space
   needs its own accounting; do not label an input bound a total allocation quota.
3. Keep recovered catalogue records historical: a persisted state is not a fresh
   durability acknowledgement. Add real device/provider interruption/resource
   acceptance and broader malformed/huge-graph cases. Source grants and model
   runtimes remain retired after restore. No automatic activation or overwriting
   of the healthy current library is permitted.
4. Other original backend gaps remain open: actual eligible digital-ink recogniser
   integration, full authoring/configuration/history, complete canvas/export,
   scheduling, PDF/interaction acceptance and requirement-by-requirement closure.
   Do not open the gate because restore component tests pass.
