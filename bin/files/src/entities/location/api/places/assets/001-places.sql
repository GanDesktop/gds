CREATE TABLE locations (
    kind TEXT NOT NULL CHECK (kind IN ('favorite', 'recent', 'pin')),
    path BLOB NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (kind, path)
);
-- #[toasty::breakpoint]
CREATE INDEX location_order ON locations(kind, position);
-- #[toasty::breakpoint]
CREATE TABLE directory_views (
    path BLOB PRIMARY KEY NOT NULL,
    columns TEXT,
    view_mode TEXT CHECK (view_mode IN ('list', 'grid'))
);
