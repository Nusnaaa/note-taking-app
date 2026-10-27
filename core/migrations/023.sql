-- Content-free native import publication receipts. An acknowledged operation
-- must remain distinguishable from a retry after a lost transport response.
CREATE TABLE native_imports(
 operation TEXT PRIMARY KEY CHECK(length(operation)=36),
 digest TEXT NOT NULL CHECK(length(digest)=64 AND digest NOT GLOB '*[^0-9a-f]*'),
 payload TEXT NOT NULL CHECK(json_valid(payload))
) STRICT;
PRAGMA user_version=23;
