# ADR 015 — cropped-image derivative pixels

Status: recorded backend engineering decision; full scene/PDF/device acceptance remains open.

The existing image model retains original PNG/JPEG bytes independently of a normalised crop. Markdown/assets derivative export previously rejected every cropped image. For a cropped image, locally decode the image asset and emit a new PNG containing the selected pixels. Keep the original asset unchanged in native storage. Export asset names include both source asset and block identity, so two crops of one image cannot overwrite each other. Explicit source-ink inclusion still runs for image blocks carrying original ink.

The crop uses the decoded source pixel grid. Retain only complete source pixels inside the selected rectangle: ceil the left/top and floor the right/bottom edges. Report inward rounding; if no complete pixel remains, return ReviewRequired. An EXIF orientation other than NoTransforms also requires review because this backend has no approved H.I. convention for applying a crop before/after metadata rotation. There is no silent rotation or expansion beyond the reviewed crop.

Output is 8-bit RGBA PNG from decoded pixels. Original compressed bytes, ancillary metadata, trailing data and profiles are not copied into the derivative. Report this fidelity limitation. The cropped bitmap may have quantisation/colour-management differences; no forensic-redaction or complete photographic/PDF fidelity claim is made. PNG/JPEG source bytes and native crop metadata remain available through their existing authorised native paths.

Use pinned `image` 0.25.10 with PNG/JPEG features, already present transitively in Cargo.lock; only stem-core's direct dependency edge changes. Its cached package manifest declares MIT OR Apache-2.0. No model resources are involved.

Current implementation admission: 32 MiB encoded input/output, 16,384 per dimension, 64 MiB declared decoder output and RGBA pixel storage. Set decoder limits, inspect declared output before decoding, and enforce encoded output length through the writer. These are admission bounds, not latency/SLOs or a hard total allocation quota: decoder metadata/internal allocations, conversions and library overhead remain part of the open resource-isolation/allocation gap. JobControl is checked before and after codec work; a running codec call is not forcibly terminated.

No persistent schema change is required. Whole-note PDF image rendering, full canvas layout/rotation fidelity, fractional-pixel resampling, orientation/crop H.I. conventions, colour management, hard worker/allocation isolation and actual-device acceptance remain open.
