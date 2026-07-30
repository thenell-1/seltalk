// NOTE 数据库模块：SQLite 初始化与基础 CRUD
// 数据库文件位于 %APPDATA%\CreativeInputMethod\data.db
// 表结构：history（历史回复）、habits（习惯记忆）、prompts（Prompt 模板）

use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// 历史回复记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: Option<i64>,
    pub captured_text: String,
    pub reply_text: String,
    pub adopted: bool,
    pub llm_mode: String,
    pub created_at: String,
}

/// 数据库连接封装（线程安全）
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// 初始化数据库（创建文件 + 建表）
    pub fn init(app: &AppHandle) -> AppResult<Self> {
        let path = db_path(app)?;
        let conn = Connection::open(&path)
            .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // 启用 WAL 模式提升并发性能
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // 创建表
        conn.execute_batch(CREATE_TABLES_SQL)
            .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        tracing::info!("数据库已初始化: {}", path.display());
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 插入历史回复记录
    pub fn insert_history(&self, record: &HistoryRecord) -> AppResult<i64> {
        let conn = self.conn.lock().map_err(|e| {
            AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;
        conn.execute(
            INSERT_HISTORY_SQL,
            params![
                record.captured_text,
                record.reply_text,
                record.adopted,
                record.llm_mode,
                record.created_at,
            ],
        )
        .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(conn.last_insert_rowid())
    }

    /// 标记回复为已采纳
    pub fn mark_adopted(&self, id: i64) -> AppResult<()> {
        let conn = self.conn.lock().map_err(|e| {
            AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;
        conn.execute(
            "UPDATE history SET adopted = 1 WHERE id = ?1",
            params![id],
        )
        .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }

    /// 分页查询历史记录
    pub fn list_history(&self, page: u32, page_size: u32) -> AppResult<Vec<HistoryRecord>> {
        let conn = self.conn.lock().map_err(|e| {
            AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;
        let offset = (page * page_size) as i64;
        let limit = page_size as i64;
        let mut stmt = conn
            .prepare(LIST_HISTORY_SQL)
            .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        let rows = stmt
            .query_map(params![limit, offset], |row| {
                Ok(HistoryRecord {
                    id: Some(row.get(0)?),
                    captured_text: row.get(1)?,
                    reply_text: row.get(2)?,
                    adopted: row.get(3)?,
                    llm_mode: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|e| {
                AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
            })?);
        }
        Ok(records)
    }

    /// 统计已采纳回复总数
    pub fn count_adopted(&self) -> AppResult<u64> {
        let conn = self.conn.lock().map_err(|e| {
            AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM history WHERE adopted = 1", [], |row| {
                row.get(0)
            })
            .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(count as u64)
    }
}

/// 获取数据库文件路径
fn db_path(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Config(format!("获取配置目录失败: {e}")))?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir.join("data.db"))
}

/// 获取当前 UTC 时间字符串
pub fn now_utc_string() -> String {
    let now: DateTime<Utc> = Utc::now();
    now.to_rfc3339()
}

const CREATE_TABLES_SQL: &str = "
CREATE TABLE IF NOT EXISTS history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    captured_text TEXT NOT NULL,
    reply_text TEXT NOT NULL,
    adopted INTEGER NOT NULL DEFAULT 0,
    llm_mode TEXT NOT NULL DEFAULT 'cloud',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_history_created_at ON history(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_adopted ON history(adopted);

CREATE TABLE IF NOT EXISTS habits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    word TEXT NOT NULL UNIQUE,
    adopt_count INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT,
    decay_weight REAL NOT NULL DEFAULT 1.0
);
CREATE INDEX IF NOT EXISTS idx_habits_count ON habits(adopt_count DESC);

CREATE TABLE IF NOT EXISTS prompts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    content TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
";

const INSERT_HISTORY_SQL: &str = "
INSERT INTO history (captured_text, reply_text, adopted, llm_mode, created_at)
VALUES (?1, ?2, ?3, ?4, ?5)
";

const LIST_HISTORY_SQL: &str = "
SELECT id, captured_text, reply_text, adopted, llm_mode, created_at
FROM history
ORDER BY created_at DESC
LIMIT ?1 OFFSET ?2
";
