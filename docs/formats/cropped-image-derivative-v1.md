# Cropped-image derivative, version 1

Existing `Content::Image` stores an asset UUID, caption and optional normalised crop rectangle. Native data and stored asset bytes are unchanged.

For Markdown/assets export, a crop emits `image-<asset UUID>-crop-<block UUID>.png` and references that file from the captioned image link. The PNG contains only the selected complete source pixels, re-encoded as 8-bit RGBA without the original metadata/trailing bytes. Multiple blocks can refer to distinct crops of the same original asset.

Bounds use the decoded source pixel grid. Fractional bounds round inward and produce a warning. For example, a four-pixel-wide image cropped from x=0.1 to x=0.9 emits the two complete central pixels. A crop containing no complete pixel or requiring an undefined EXIF rotation convention returns ReviewRequired. Malformed/over-admission images fail before a publishable bundle is returned. Fresh document/block protection and final plan-generation/confirmation checks continue to apply.

An uncropped image retains the existing byte-preserving export behaviour. See ADR 015 for fidelity, codec and resource limitations. PDF/full scene integration remains open.
