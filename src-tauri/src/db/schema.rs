// NOTE 集中所有建表 DDL，init_db 时一次性执行
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS words (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    word        TEXT    NOT NULL,
    category    TEXT    NOT NULL DEFAULT '',
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_words_category ON words(category);
CREATE INDEX IF NOT EXISTS idx_words_enabled ON words(enabled);

CREATE TABLE IF NOT EXISTS prompts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    template    TEXT    NOT NULL,
    is_default  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS word_freq (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    word         TEXT    NOT NULL UNIQUE,
    count        INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT
);

CREATE TABLE IF NOT EXISTS window_state (
    label          TEXT PRIMARY KEY,
    x              INTEGER NOT NULL,
    y              INTEGER NOT NULL,
    w              INTEGER NOT NULL,
    h              INTEGER NOT NULL,
    always_on_top  INTEGER NOT NULL DEFAULT 1
);
"#;
