// TODO 人工审查点：1.目录创建 2.WAL 模式 3.种子数据幂等
// NOTE 数据库初始化 + 种子数据；子模块按表分组
pub mod prompts;
pub mod schema;
pub mod settings;
pub mod window_state;
pub mod word_freq;
pub mod words;

use std::path::Path;

use rusqlite::Connection;

use crate::error::AppResult;

/// 默认 Prompt 模板（首次启动写入）
const DEFAULT_PROMPT_TEMPLATE: &str = r#"你是一个聊天回复助手。请根据下面的对话上下文，生成 {{n}} 条简短、自然、口语化的回复候选。每条回复独占一行，用 --- 分隔。不要编号，不要额外说明。

对话上下文：
{{origin}}

参考词库（可酌情使用）：
{{words}}"#;

/// 打开/创建数据库并执行建表
pub fn init_db(path: &Path) -> AppResult<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    conn.execute_batch(schema::SCHEMA_SQL)?;
    tracing::info!("数据库已初始化: {}", path.display());
    Ok(conn)
}

/// 表空时写入默认 Prompt 模板（幂等）
pub fn seed_if_empty(conn: &Connection) -> AppResult<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM prompts", [], |r| r.get(0))?;
    if count == 0 {
        let ts = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO prompts (name, template, is_default, created_at, updated_at)
             VALUES (?1, ?2, 1, ?3, ?3)",
            rusqlite::params!["默认回复模板", DEFAULT_PROMPT_TEMPLATE, ts],
        )?;
        tracing::info!("已写入默认 Prompt 模板");
    }
    Ok(())
}
