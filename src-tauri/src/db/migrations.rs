// TODO 人工审查点：1.迁移幂等性 2.事务边界 3.版本号单调递增 4.向后兼容 5.失败回滚
// NOTE 数据库版本迁移：基于 settings.schema_version 记录已应用版本，按顺序应用增量 SQL
//       设计原则：
//       - SCHEMA_SQL 是"基线"：包含最新表结构（CREATE TABLE IF NOT EXISTS 幂等）
//         全新数据库执行 SCHEMA_SQL 后即处于 LATEST_VERSION，无需任何迁移
//       - MIGRATIONS 是"增量"：老用户从旧版本升级时，按顺序应用 version > current 的迁移
//       - 每个迁移在独立事务中执行，失败回滚（保留旧版本号，下次启动重试）
//       - 维护方式：添加字段时，①更新 SCHEMA_SQL 中的表 DDL ②在 MIGRATIONS 末尾追加 ALTER TABLE
use rusqlite::{params, Connection};

use crate::error::AppResult;

/// 单个增量迁移定义
#[derive(Debug)]
pub struct Migration {
    /// 应用此迁移后的目标版本号（必须严格单调递增，从 1 开始）
    pub version: u32,
    /// 迁移名（用于日志，简短描述）
    pub name: &'static str,
    /// SQL 语句（必须幂等：重复执行不报错，如用 IF NOT EXISTS / IF EXISTS）
    pub up_sql: &'static str,
}

/// settings 表中存储 schema_version 的键名
const SCHEMA_VERSION_KEY: &str = "schema_version";

/// 当前最新 schema 版本号
///
/// 含义：执行完 SCHEMA_SQL + 全部 MIGRATIONS 后的版本号。
/// - 全新数据库：SCHEMA_SQL 已是最新结构 → 直接设为 LATEST_VERSION
/// - 老用户：从 current_version 开始，按序应用 MIGRATIONS，最终达到 LATEST_VERSION
pub const LATEST_VERSION: u32 = 2;

/// 增量迁移列表（按 version 升序排列）
///
/// 当前为空：项目首个正式版本所有表结构已包含在 SCHEMA_SQL 中。
/// 未来添加字段/索引时，按以下步骤维护：
/// 1. 更新 SCHEMA_SQL 中对应表的 DDL（保证新用户直接拥有最新结构）
/// 2. 在本数组末尾追加 Migration（version 递增，up_sql 用 ALTER TABLE 等）
/// 3. 更新 LATEST_VERSION 常量为新的最大版本号
pub const MIGRATIONS: &[Migration] = &[
    // v2：prompts 表新增 tags 列（逗号分隔标签，用于模板分类）
    // 老用户（v1）执行 ALTER TABLE 加列；新用户由 SCHEMA_SQL 基线直接拥有该列
    Migration {
        version: 2,
        name: "add_tags_to_prompts",
        up_sql: "ALTER TABLE prompts ADD COLUMN tags TEXT NOT NULL DEFAULT ''",
    },
];

/// 读取当前 schema 版本号
///
/// - 若 settings 表中无 schema_version 键 → 返回 0（视为"项目首版之前"）
/// - 若值非法（非数字）→ 返回 0 并记录警告
pub fn current_version(conn: &Connection) -> AppResult<u32> {
    let result: rusqlite::Result<Option<String>> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![SCHEMA_VERSION_KEY],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        });

    match result {
        Ok(Some(s)) => match s.parse::<u32>() {
            Ok(v) => Ok(v),
            Err(_) => {
                tracing::warn!("schema_version 值非法: {s}，视为 0");
                Ok(0)
            }
        },
        Ok(None) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

/// 写入 schema 版本号（UPSERT：存在则更新，不存在则插入）
///
/// 注：可在事务内调用（传入 &Transaction，自动 Deref 为 &Connection）
pub fn set_version(conn: &Connection, version: u32) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SCHEMA_VERSION_KEY, version.to_string()],
    )?;
    Ok(())
}

/// 应用所有未应用的迁移，返回应用后的版本号
///
/// 流程：
/// 1. 读取当前版本号 current
/// 2. 全新库判定（current == 0）：SCHEMA_SQL 已是最新结构（含全部列/索引），
///    直接设为 LATEST_VERSION，**不执行 MIGRATIONS**（避免 ALTER TABLE 重复加列）
/// 3. 老用户迁移（0 < current < LATEST_VERSION）：按序遍历 MIGRATIONS，对 version > current 的迁移：
///    - 在事务中执行 up_sql
///    - 更新 schema_version 为 m.version
///    - 提交事务
/// 4. 若全部迁移后版本仍 < LATEST_VERSION，则直接将 schema_version 设为 LATEST_VERSION
///
/// 失败处理：单个迁移失败时事务回滚，schema_version 保留旧值，下次启动可重试
pub fn apply_migrations(conn: &Connection) -> AppResult<u32> {
    let mut current = current_version(conn)?;
    tracing::info!("当前 schema 版本: {current}, 最新版本: {LATEST_VERSION}");

    if current > LATEST_VERSION {
        // 异常情况：数据库版本号高于代码版本（可能降级运行）
        // 不做降级迁移，仅记录警告，由人工处理
        tracing::warn!(
            "数据库版本 {current} 高于代码版本 {LATEST_VERSION}，可能存在降级运行，跳过迁移"
        );
        return Ok(current);
    }

    // 全新库（current == 0）：SCHEMA_SQL 已执行（表结构为最新），直接设为 LATEST_VERSION
    // 不执行 MIGRATIONS，避免 ALTER TABLE ADD COLUMN 与 SCHEMA_SQL 中的列定义冲突
    // （场景：init_db 先执行 SCHEMA_SQL 建表，再调用 apply_migrations）
    if current == 0 {
        set_version(conn, LATEST_VERSION)?;
        tracing::info!("全新数据库，schema 版本直接设为 LATEST_VERSION={LATEST_VERSION}");
        return Ok(LATEST_VERSION);
    }

    for m in MIGRATIONS {
        if m.version <= current {
            continue; // 已应用，跳过
        }
        tracing::info!("应用数据库迁移 v{}: {}", m.version, m.name);

        let tx = conn.unchecked_transaction()?;
        // SQL 执行失败 → 回滚事务，保留旧版本号，下次启动可重试
        if let Err(e) = tx.execute_batch(m.up_sql) {
            tracing::error!("迁移 v{} SQL 执行失败: {e}", m.version);
            let _ = tx.rollback();
            return Err(e.into());
        }
        if let Err(e) = set_version(&tx, m.version) {
            tracing::error!("迁移 v{} 更新版本号失败: {e}", m.version);
            let _ = tx.rollback();
            return Err(e);
        }
        tx.commit()?;
        current = m.version;
        tracing::info!("迁移 v{} 完成", m.version);
    }

    // 全部迁移应用完毕，若仍低于 LATEST_VERSION，则一次性提升到 LATEST
    // （场景：老用户所有迁移已应用但版本号未追上 LATEST，或 MIGRATIONS 为空）
    if current < LATEST_VERSION {
        set_version(conn, LATEST_VERSION)?;
        current = LATEST_VERSION;
        tracing::info!("schema 版本已提升至 LATEST_VERSION={LATEST_VERSION}");
    }

    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;
    use rusqlite::Connection;

    /// 构造内存数据库（已应用全部 SCHEMA_SQL）
    fn new_mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn
    }

    // ===== 正常流程 =====

    #[test]
    fn test_current_version_default_zero() {
        // 全新数据库：settings 表存在但无 schema_version 键 → 返回 0
        let conn = new_mem_db();
        assert_eq!(current_version(&conn).unwrap(), 0);
    }

    #[test]
    fn test_set_and_read_version() {
        let conn = new_mem_db();
        set_version(&conn, 5).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 5);
    }

    #[test]
    fn test_set_version_upsert() {
        // 重复写入应更新而非报错
        let conn = new_mem_db();
        set_version(&conn, 1).unwrap();
        set_version(&conn, 2).unwrap();
        set_version(&conn, 3).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 3);
    }

    #[test]
    fn test_apply_migrations_fresh_db() {
        // 全新数据库：无任何迁移需应用，但应直接提升到 LATEST_VERSION
        let conn = new_mem_db();
        let final_version = apply_migrations(&conn).unwrap();
        assert_eq!(final_version, LATEST_VERSION);
        assert_eq!(current_version(&conn).unwrap(), LATEST_VERSION);
    }

    #[test]
    fn test_apply_migrations_idempotent() {
        // 多次调用 apply_migrations 应幂等，不报错
        let conn = new_mem_db();
        apply_migrations(&conn).unwrap();
        let v1 = current_version(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        let v2 = current_version(&conn).unwrap();
        assert_eq!(v1, v2);
        assert_eq!(v2, LATEST_VERSION);
    }

    // ===== 边界场景 =====

    #[test]
    fn test_invalid_version_value_treated_as_zero() {
        // settings.schema_version 值非法（非数字）→ 视为 0
        let conn = new_mem_db();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('schema_version', 'not-a-number')",
            [],
        )
        .unwrap();
        assert_eq!(current_version(&conn).unwrap(), 0);
    }

    #[test]
    fn test_apply_migrations_with_future_version() {
        // 数据库版本高于 LATEST_VERSION：应跳过迁移，返回当前版本（不降级）
        let conn = new_mem_db();
        set_version(&conn, LATEST_VERSION + 100).unwrap();
        let v = apply_migrations(&conn).unwrap();
        assert_eq!(v, LATEST_VERSION + 100);
        // 不应被回退
        assert_eq!(current_version(&conn).unwrap(), LATEST_VERSION + 100);
    }

    #[test]
    fn test_apply_migrations_zero_version_promotes_to_latest() {
        // 版本 0（首版之前）且无迁移需应用 → 直接提升到 LATEST
        let conn = new_mem_db();
        // 不设置 schema_version，默认 0
        let v = apply_migrations(&conn).unwrap();
        assert_eq!(v, LATEST_VERSION);
    }

    #[test]
    fn test_set_version_in_transaction() {
        // 在事务中调用 set_version 应正常工作
        let conn = new_mem_db();
        let tx = conn.unchecked_transaction().unwrap();
        set_version(&tx, 7).unwrap();
        tx.commit().unwrap();
        assert_eq!(current_version(&conn).unwrap(), 7);
    }

    // ===== 迁移失败回滚测试（模拟）=====

    #[test]
    fn test_migration_failure_rolls_back_version() {
        // 通过手动模拟一个失败的 SQL，验证版本号不被写入
        let conn = new_mem_db();
        set_version(&conn, 0).unwrap();

        // 模拟一个失败的迁移：SQL 语法错误
        let tx = conn.unchecked_transaction().unwrap();
        let result = tx.execute_batch("THIS IS NOT VALID SQL");
        assert!(result.is_err(), "SQL 应执行失败");

        // 关键：未调用 set_version，回滚后版本号应保持原值
        let _ = tx.rollback();
        assert_eq!(current_version(&conn).unwrap(), 0);
    }

    #[test]
    fn test_migration_success_commits_version() {
        // 模拟一个成功的迁移：执行合法 SQL + 更新版本号 + 提交
        let conn = new_mem_db();
        set_version(&conn, 0).unwrap();

        let tx = conn.unchecked_transaction().unwrap();
        // 合法 SQL（创建一个临时表，模拟迁移产物）
        tx.execute_batch("CREATE TABLE IF NOT EXISTS _migration_test (id INTEGER);")
            .unwrap();
        set_version(&tx, 42).unwrap();
        tx.commit().unwrap();

        // 版本号应已写入
        assert_eq!(current_version(&conn).unwrap(), 42);
        // 迁移产物应存在
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM _migration_test", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    // ===== v1→v2 迁移：prompts 表加 tags 列 =====

    /// 构造一个"老用户 v1 数据库"：建表时不含 tags 列，schema_version=1
    fn new_v1_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // 旧版表结构（无 tags 列）
        conn.execute_batch(
            r#"
            CREATE TABLE prompts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                template TEXT NOT NULL,
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            "#,
        )
        .unwrap();
        set_version(&conn, 1).unwrap();
        // 插入一条旧数据（无 tags）
        conn.execute(
            "INSERT INTO prompts (name, template, is_default, created_at, updated_at)
             VALUES ('旧模板', '内容', 1, '2026-01-01', '2026-01-01')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_v2_migration_adds_tags_column() {
        // 老用户 v1 → 应用迁移 → 应加上 tags 列且版本到 2
        let conn = new_v1_db();
        assert_eq!(current_version(&conn).unwrap(), 1);

        let final_version = apply_migrations(&conn).unwrap();
        assert_eq!(final_version, 2);

        // tags 列应存在且旧数据默认值为空串
        let tags: String = conn
            .query_row("SELECT tags FROM prompts WHERE name = '旧模板'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tags, "");
    }

    #[test]
    fn test_v2_migration_idempotent() {
        // 迁移应用后再次调用应无副作用
        let conn = new_v1_db();
        apply_migrations(&conn).unwrap();
        let v1 = current_version(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        let v2 = current_version(&conn).unwrap();
        assert_eq!(v1, v2);
        assert_eq!(v2, LATEST_VERSION);
    }
}
