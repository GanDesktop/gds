CREATE TABLE IF NOT EXISTS quick_access (
    location_kind TEXT NOT NULL,
    identity TEXT NOT NULL,
    path BLOB NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (location_kind, identity, path)
);
-- #[toasty::breakpoint]
CREATE INDEX IF NOT EXISTS quick_access_order ON quick_access(position);
-- #[toasty::breakpoint]
CREATE TABLE IF NOT EXISTS places_meta (
    key TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL
);
