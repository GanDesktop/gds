CREATE TABLE saved_locations_with_pins (
    kind TEXT NOT NULL CHECK (kind IN ('favorite', 'recent', 'pin')),
    location_kind TEXT NOT NULL,
    identity TEXT NOT NULL,
    path BLOB NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (kind, location_kind, identity, path)
);
-- #[toasty::breakpoint]
INSERT INTO saved_locations_with_pins SELECT * FROM saved_locations;
-- #[toasty::breakpoint]
INSERT INTO saved_locations_with_pins (kind, location_kind, identity, path, position)
SELECT kind, 'directory', '', path, position FROM locations WHERE kind = 'pin';
-- #[toasty::breakpoint]
DROP TABLE saved_locations;
-- #[toasty::breakpoint]
ALTER TABLE saved_locations_with_pins RENAME TO saved_locations;
-- #[toasty::breakpoint]
CREATE INDEX saved_location_order ON saved_locations(kind, position);
-- #[toasty::breakpoint]
DELETE FROM locations WHERE kind = 'pin';
