CREATE TABLE capture_receipts(ticket TEXT PRIMARY KEY, block TEXT NOT NULL REFERENCES blocks(id), fingerprint TEXT NOT NULL) STRICT;
PRAGMA user_version=5;
