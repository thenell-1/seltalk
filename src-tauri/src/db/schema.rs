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
    tags        TEXT    NOT NULL DEFAULT '',           -- 逗号分隔标签（如 "简短,正式"）
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

-- 历史记录表：用户选中的候选回复（按时间倒序查询）
CREATE TABLE IF NOT EXISTS history (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    origin       TEXT    NOT NULL,                  -- 原始识别文本（脱敏后）
    selected     TEXT    NOT NULL,                  -- 用户选中的候选文本
    prompt_name  TEXT    NOT NULL DEFAULT '',       -- 当时的 Prompt 模板名（空表示未知）
    model        TEXT    NOT NULL DEFAULT '',       -- 当时的 LLM 模型
    created_at   TEXT    NOT NULL                   -- 选择时间（RFC3339）
);
CREATE INDEX IF NOT EXISTS idx_history_created_at ON history(created_at DESC);

-- LLM 配置档案表：多份命名 LLM 配置，is_active 标记当前生效配置（同 prompts.is_default）
-- api_key 列通过 Windows DPAPI 加密存储（见 db/llm_profiles.rs），明文不落库
CREATE TABLE IF NOT EXISTS llm_profiles (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    name                TEXT    NOT NULL,
    base_url            TEXT    NOT NULL DEFAULT '',
    api_key             TEXT    NOT NULL DEFAULT '',           -- DPAPI 加密存储
    model               TEXT    NOT NULL DEFAULT '',
    model_type          TEXT    NOT NULL DEFAULT '',           -- openai/anthropic/azure/deepseek/local...
    temperature         REAL    NOT NULL DEFAULT 0.6,
    max_tokens          INTEGER NOT NULL DEFAULT 1024,
    max_context_length  INTEGER NOT NULL DEFAULT 0,            -- 0 = 未设置/不限
    stream_enabled      INTEGER NOT NULL DEFAULT 1,
    is_active           INTEGER NOT NULL DEFAULT 0,            -- 当前生效配置（互斥，同 prompts.is_default）
    created_at          TEXT    NOT NULL,
    updated_at          TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_llm_profiles_active ON llm_profiles(is_active);
"#;
