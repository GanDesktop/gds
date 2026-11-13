CREATE TABLE saved_locations (
    kind TEXT NOT NULL CHECK (kind IN ('favorite', 'recent', 'pin')),
    location_kind TEXT NOT NULL,
    identity TEXT NOT NULL,
    path BLOB NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (kind, location_kind, identity, path)
);
-- #[toasty::breakpoint]
CREATE INDEX saved_location_order ON saved_locations(kind, position);
-- #[toasty::breakpoint]
CREATE TABLE view_settings (
    key_kind TEXT NOT NULL,
    key_identity TEXT NOT NULL,
    key_path BLOB NOT NULL,
    columns TEXT,
    widths TEXT,
    sort TEXT,
    details INTEGER,
    view_mode TEXT CHECK (view_mode IN ('list', 'grid')),
    PRIMARY KEY (key_kind, key_identity, key_path)
);
