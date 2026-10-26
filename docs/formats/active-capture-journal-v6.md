# Active capture journal v6

Private recovery files use magic `STEMAC06` and the existing checksum/file publication
protocol. This is not notebook interchange or an additional original-ink authority.
All v5 fields retain their byte order. Immediately after the layer selector and
optional custom-layer UUID, write a strict boolean cell-presence byte. If true,
write block UUID (two big-endian 64-bit words), cell UUID and a big-endian signed
64-bit nonnegative expected document revision. Coordinate-space length/data and
observed points follow unchanged. Layer and cell selectors are mutually exclusive.
The checksum covers the complete payload, including routing metadata.

Readers accept known versions 01–06. Old versions have no cell-presence field and
receive no invented cell binding. V5 retains its pressure-response flag/values;
v3/v4 retain their previous tool/layer layouts. Unknown tags, invalid booleans,
negative revisions, trailing bytes, malformed UTF-8 or checksum mismatches fail
without adopting or deleting authoritative evidence.

Record frame revision and expected document revision are distinct. Prefix extension
must preserve entity/library/source space, immutable stroke identity/style and the
cell or layer binding. Explicit reviewed cell retargeting can change only private
routing and frame review metadata after fresh native destination/receipt checks.
A stale destination is retained for review, never automatically rebased or converted
to a new block. CaptureCell JSON includes the expected entity, verified inside the
native SQLite capture transaction. Source observations remain exact.
