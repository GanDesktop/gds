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
-- #[toasty::breakpoint]
INSERT INTO view_settings (key_kind, key_identity, key_path, columns, view_mode)
SELECT 'directory', '', path, columns, view_mode
FROM directory_views;
-- #[toasty::breakpoint]
DROP TABLE directory_views;
