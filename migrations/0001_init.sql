PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS objects (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    title TEXT,
    source_json TEXT NOT NULL,
    lifecycle TEXT NOT NULL DEFAULT 'available',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS representations (
    id TEXT PRIMARY KEY,
    object_id TEXT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    mime_type TEXT NOT NULL,
    role TEXT NOT NULL,
    storage_kind TEXT NOT NULL,
    storage_json TEXT NOT NULL,
    size_bytes INTEGER,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_representations_object ON representations(object_id);
CREATE INDEX IF NOT EXISTS idx_representations_mime ON representations(mime_type);

CREATE TABLE IF NOT EXISTS pages (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    position INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS page_items (
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    object_id TEXT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    x REAL NOT NULL DEFAULT 0,
    y REAL NOT NULL DEFAULT 0,
    width REAL NOT NULL DEFAULT 220,
    height REAL NOT NULL DEFAULT 150,
    z_index INTEGER NOT NULL DEFAULT 0,
    pinned INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(page_id, object_id)
);
CREATE INDEX IF NOT EXISTS idx_page_items_page_z ON page_items(page_id, z_index);

CREATE TABLE IF NOT EXISTS plugins (
    id TEXT PRIMARY KEY,
    manifest_json TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY,
    plugin_id TEXT,
    action_id TEXT NOT NULL,
    source_object_id TEXT REFERENCES objects(id) ON DELETE SET NULL,
    target_object_id TEXT REFERENCES objects(id) ON DELETE SET NULL,
    state TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
