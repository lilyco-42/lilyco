CREATE TABLE IF NOT EXISTS records (
    id INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id BETWEEN 1 AND 4294967295),
    collection TEXT NOT NULL,
    payload TEXT NOT NULL CHECK (json_valid(payload))
) STRICT;

CREATE INDEX IF NOT EXISTS records_by_collection ON records (collection, id);
