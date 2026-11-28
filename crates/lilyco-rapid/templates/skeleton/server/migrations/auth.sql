CREATE TABLE IF NOT EXISTS rapid_users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'viewer',
    display_name TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS rapid_roles (
    name TEXT PRIMARY KEY,
    description TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS rapid_permissions (
    role TEXT NOT NULL,
    action TEXT NOT NULL,
    object TEXT NOT NULL,
    PRIMARY KEY (role, action, object)
);

INSERT OR IGNORE INTO rapid_roles (name, description) VALUES
    ('admin', 'Full access'),
    ('viewer', 'Read-only access');