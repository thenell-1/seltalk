// TODO 人工审查点：1.目录创建 2.WAL 模式 3.种子数据幂等 4.迁移幂等性 5.连接池配置
// NOTE 数据库初始化 + 种子数据；子模块按表分组
pub mod history;
pub mod llm_profiles;
pub mod migrations;
pub mod prompts;
pub mod schema;
pub mod settings;
pub mod window_state;
pub mod word_freq;
pub mod words;

use std::path::Path;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use crate::error::{AppError, AppResult};
use crate::state::DbPool;

/// 默认 Prompt 模板（首次启动写入）
const DEFAULT_PROMPT_TEMPLATE: &str = r#"你是一个聊天回复助手。请根据下面的对话上下文，生成 {{n}} 条简短、自然、口语化的回复候选。每条回复独占一行，用 --- 分隔。不要编号，不要额外说明。

对话上下文：
{{origin}}

参考词库（可酌情使用）：
{{words}}"#;

/// 打开/创建数据库并执行建表 + 版本迁移，返回连接池
///
/// 流程：
/// 1. 创建目录（缺失时自动创建）
/// 2. 构造 SqliteConnectionManager + 连接池（max_size=8）
/// 3. 取首个连接：开启 WAL 模式 + 外键约束 + 执行 SCHEMA_SQL 基线建表 + 应用增量迁移
/// 4. 归还连接到池，返回 Pool
///
/// P1.1：从单 Mutex<Connection> 改为 Pool<SqliteConnectionManager>，
///       WAL 模式下读操作可并发（不阻塞写），提升管理面板查询时不阻塞主链路的并发能力
pub fn init_db(path: &Path) -> AppResult<DbPool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // 构造连接管理器（SQLite 文件）
    let manager = SqliteConnectionManager::file(path);
    // 连接池配置：max_size=8 足以并发管理面板查询 + 主链路写入
    let pool: DbPool = Pool::builder()
        .max_size(8)
        .min_idle(Some(1))
        .build(manager)
        .map_err(|e| AppError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;

    // 取首个连接执行初始化：WAL + 外键 + 建表 + 迁移
    {
        let conn = pool
            .get()
            .map_err(|e| AppError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
        // PRAGMA journal_mode=WAL 是数据库级别的（持久化到文件头），一次设置永久生效
        // PRAGMA foreign_keys=ON 是连接级别的，但 SelTalk 业务无 FK 约束，可不每个连接设置
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(schema::SCHEMA_SQL)?;
        let final_version = migrations::apply_migrations(&conn)?;
        tracing::info!(
            "数据库已初始化: {} (schema_version={})",
            path.display(),
            final_version
        );
    }

    Ok(pool)
}

/// 表空时写入默认 Prompt 模板（幂等）
///
/// 注：接受 &Connection 兼容旧 API；调用方需从 Pool 取连接后传入
pub fn seed_if_empty(conn: &rusqlite::Connection) -> AppResult<()> {
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
