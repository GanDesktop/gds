CREATE TABLE cache_sessions (
    namespace TEXT NOT NULL, session TEXT NOT NULL, snapshot TEXT NOT NULL,
    frontier INTEGER NOT NULL, snapshot_bytes INTEGER NOT NULL,
    touched INTEGER NOT NULL DEFAULT 0, bytes INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(namespace, session)
);
-- #[toasty::breakpoint]
CREATE TABLE cache_pages (
    namespace TEXT NOT NULL, session TEXT NOT NULL, upper INTEGER NOT NULL,
    start INTEGER NOT NULL, before INTEGER, context TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    PRIMARY KEY(namespace, session, upper)
);
-- #[toasty::breakpoint]
CREATE TABLE cache_events (
    namespace TEXT NOT NULL, session TEXT NOT NULL, cursor INTEGER NOT NULL,
    payload TEXT NOT NULL, bytes INTEGER NOT NULL,
    PRIMARY KEY(namespace, session, cursor)
);
-- #[toasty::breakpoint]
CREATE INDEX cache_lru ON cache_sessions(touched);
