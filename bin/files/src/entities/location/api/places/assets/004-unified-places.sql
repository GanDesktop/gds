CREATE TABLE saved_locations (
    kind TEXT NOT NULL CHECK (kind IN ('favorite', 'recent')),
    location_kind TEXT NOT NULL,
    identity TEXT NOT NULL,
    path BLOB NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (kind, location_kind, identity, path)
);
-- #[toasty::breakpoint]
CREATE INDEX saved_location_order ON saved_locations(kind, position);
-- #[toasty::breakpoint]
INSERT INTO saved_locations (kind, location_kind, identity, path, position)
SELECT kind, 'directory', '', path, position FROM locations WHERE kind != 'pin';
-- #[toasty::breakpoint]
INSERT INTO saved_locations (kind, location_kind, identity, path, position)
SELECT 'favorite', 'remote', uri, X'', position +
    (SELECT COALESCE(MAX(position), 0) FROM locations WHERE kind = 'favorite')
FROM remote_locations WHERE favorite != 0;
-- #[toasty::breakpoint]
INSERT INTO saved_locations (kind, location_kind, identity, path, position)
SELECT 'recent', 'remote', uri, X'', position +
    (SELECT COALESCE(MAX(position), 0) FROM locations WHERE kind = 'recent')
FROM remote_locations WHERE recent != 0;
-- #[toasty::breakpoint]
DELETE FROM locations WHERE kind != 'pin';
-- #[toasty::breakpoint]
DROP TABLE remote_locations;
