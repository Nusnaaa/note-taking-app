# ADR 013 — explicit source-space whole-stroke erasure

Status: recorded engineering decision for the I0/I1 backend increment; no production H.I. acceptance.

The existing explicit stroke-ID erasure already changes visibility atomically and retains original observations independently of Undo retirement. Add an `EraseGesture` authoring command for one block, exact current revision, named source space, positive radius and finite polyline. Its swept radius selects whole strokes by observed centreline distance, including singleton/repeated points and crossing segments. Contacted strokes hide together in the existing single reversible operation. A miss returns the unchanged block without revising it or consuming Undo. Existing source, capture receipts, styles and recognition payloads are not rewritten.

The caller must explicitly map a gesture into the recorded source space. The core does not infer display calibration, apply block/view transforms a second time, interpret raw eraser observations as commands, or infer legacy nib width. Hidden blocks and editable presentation reject this source-ink operation. All visible candidate source spaces must match; stale revision, fresh protection, malformed input, geometry overflow, work admission or storage failure publish no prefix of hits.

Current implementation admission limits are 4,096 gesture points, 4,096 visible candidate strokes and 1,000,000 reserved segment-pair checks per request. These bound this synchronous geometry operation; they are not throughput, latency or document-size SLOs. Entire candidate pair work is reserved even when an early segment hits. These limits do not close the broader individual-record/SQLite allocation gap.

This increment covers one explicitly targeted block and whole-stroke centreline selection. Nib-outline edge contact, partial-segment erasure, selection-mode gestures, compound gestures across blocks/views, H.I. gesture transactions, source/device coordinate mapping, full scene integration and device acceptance remain open. An eraser trace retained in source ink remains an observation, not automatic destructive authority. Existing explicit selected-stroke erasure remains available.
