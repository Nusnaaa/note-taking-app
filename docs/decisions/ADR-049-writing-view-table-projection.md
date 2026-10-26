# ADR 049 — saved writing-view projection of table ink

Status: implementation; component verification recorded separately. Complete backend acceptance remains open.

A read-only host request binds the saved view ID/revision, source entity revision or explicit nested embed path, block revision and caller-supplied logical viewport. The saved camera projects the source into that viewport. Embed clips retain exact transforms; the viewport is an additional identity clip. Original ordered samples and capture metadata remain unchanged. The shared camera-to-frame operation is also used for canvas embeds.

Direct canvas reads honour actual layer visibility. Exercise views require a currently visible exercise and actual source-block membership. Standalone embed objects have no exercise membership model, so nonempty nested embed paths in exercise views are unavailable; there is no implicit master fallback. Fresh authority and scope checks run before and after page retrieval. All envelope and ink bytes share the existing 8 MiB response bound, and transformed points and clip corners must remain finite.

Saved-view and exercise reads authorise their SQL owner before payload decoding. Exercise lock metadata is checked before decoding; missing lock metadata fails closed. Payload identity is checked against SQL identity. Corrupt denied content therefore cannot become a deserialization oracle.

No schema or native-document version change is required. Physical screen calibration, flow relayout, full scene export, lasso selection, reference rebinding and actual device rendering remain separate unfinished work. The explicit viewport uses caller logical units, not inferred pixels or millimetres.
