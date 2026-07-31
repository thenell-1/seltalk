// TODO 人工审查点：1.SQL 注入防护(params!) 2.LIKE 转义 3.分页边界 4.大批量清理事务 5.ts-rs 类型导出
// NOTE 历史记录表 CRUD：记录用户选中的候选回复，支持搜索 + 分页查询 + 清理
//       场景：用户在悬浮窗选中候选 → orchestrator 异步写入 → 管理面板按时间倒序查询
//       P4.4：HistoryEntry 派生 TS，cargo test 时自动生成 .ts 到 ./bindings/db/
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::AppResult;

/// 历史记录条目（对应 history 表一行）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../bindings/db/HistoryEntry.ts")]
pub struct HistoryEntry {
    /// 记录 ID
    #[ts(type = "number | null")]
    pub id: Option<i64>,
    /// 原始识别文本（脱敏后）
    pub origin: String,
    /// 用户选中的候选文本
    pub selected: String,
    /// 当时的 Prompt 模板名（空表示未知/默认）
    pub prompt_name: String,
    /// 当时的 LLM 模型
    pub model: String,
    /// 选择时间（RFC3339）
    pub created_at: String,
}

/// 列表筛选 + 分页参数
#[derive(Debug, Clone, Default)]
pub struct HistoryFilter {
    /// 搜索关键字（模糊匹配 origin 或 selected 字段）
    pub search: Option<String>,
    /// 最大返回条数（建议 20-100）
    pub limit: u32,
    /// 偏移量（从第几条开始，0-based）
    pub offset: u32,
}

/// 单条历史记录的写入参数（无需 ID 与时间戳，由 DB 自动生成）
#[derive(Debug, Clone)]
pub struct HistoryRecord<'a> {
    pub origin: &'a str,
    pub selected: &'a str,
    pub prompt_name: &'a str,
    pub model: &'a str,
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

/// 写入一条历史记录
///
/// # 参数
/// - `conn`：数据库连接
/// - `record`：历史记录内容（origin/selected/prompt_name/model）
pub fn record(conn: &Connection, rec: &HistoryRecord) -> AppResult<()> {
    conn.execute(
        "INSERT INTO history (origin, selected, prompt_name, model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![rec.origin, rec.selected, rec.prompt_name, rec.model, now()],
    )?;
    Ok(())
}

/// 查询历史记录列表（按时间倒序，支持搜索 + 分页）
///
/// # 参数
/// - `conn`：数据库连接
/// - `filter`：筛选 + 分页参数（limit 上限 500 钳制保护）
pub fn history_list(conn: &Connection, filter: &HistoryFilter) -> AppResult<Vec<HistoryEntry>> {
    let limit = filter.limit.min(500) as i64;
    let offset = filter.offset as i64;

    let mut sql = String::from(
        "SELECT id, origin, selected, prompt_name, model, created_at
         FROM history WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ref search) = filter.search {
        if !search.is_empty() {
            // 双字段模糊匹配：origin 或 selected 任意一个命中即返回
            sql.push_str(" AND (origin LIKE ? OR selected LIKE ?) COLLATE NOCASE");
            let pattern = format!("%{search}%");
            param_values.push(Box::new(pattern.clone()));
            param_values.push(Box::new(pattern));
        }
    }
    sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?");
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let params_ref: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_ref.as_slice(), |r| {
        Ok(HistoryEntry {
            id: Some(r.get(0)?),
            origin: r.get(1)?,
            selected: r.get(2)?,
            prompt_name: r.get(3)?,
            model: r.get(4)?,
            created_at: r.get(5)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 统计历史记录总数（支持搜索条件，用于分页计算）
///
/// # 参数
/// - `conn`：数据库连接
/// - `search`：可选搜索关键字（与 history_list 同口径）
pub fn history_count(conn: &Connection, search: Option<&str>) -> AppResult<i64> {
    let mut sql = String::from("SELECT COUNT(*) FROM history WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(s) = search {
        if !s.is_empty() {
            sql.push_str(" AND (origin LIKE ? OR selected LIKE ?) COLLATE NOCASE");
            let pattern = format!("%{s}%");
            param_values.push(Box::new(pattern.clone()));
            param_values.push(Box::new(pattern));
        }
    }

    let params_ref: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let count: i64 = conn.query_row(&sql, params_ref.as_slice(), |r| r.get(0))?;
    Ok(count)
}

/// 删除单条历史记录
///
/// # 参数
/// - `conn`：数据库连接
/// - `id`：记录 ID
pub fn history_delete(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM history WHERE id = ?1", params![id])?;
    Ok(())
}

/// 清空全部历史记录（事务保证原子性）
///
/// # 参数
/// - `conn`：数据库连接
pub fn history_clear(conn: &Connection) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM history", [])?;
    // 重置 AUTOINCREMENT 计数器，避免 ID 越来越大
    tx.execute("DELETE FROM sqlite_sequence WHERE name = 'history'", [])?;
    tx.commit()?;
    tracing::info!("历史记录表已清空");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;
    use rusqlite::Connection;

    /// 构造内存数据库（避免文件残留）
    fn new_mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn
    }

    fn sample_record<'a>(origin: &'a str, selected: &'a str) -> HistoryRecord<'a> {
        HistoryRecord {
            origin,
            selected,
            prompt_name: "默认回复模板",
            model: "gpt-test",
        }
    }

    // ===== 正常流程 =====

    #[test]
    fn test_record_single() {
        let conn = new_mem_db();
        record(&conn, &sample_record("你好", "你好！")).unwrap();
        let list = history_list(
            &conn,
            &HistoryFilter {
                limit: 10,
                offset: 0,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].origin, "你好");
        assert_eq!(list[0].selected, "你好！");
        assert_eq!(list[0].prompt_name, "默认回复模板");
        assert_eq!(list[0].model, "gpt-test");
    }

    #[test]
    fn test_history_list_sorted_desc_by_time() {
        let conn = new_mem_db();
        // 顺序写入 3 条，倒序查询应返回最新写入的在前
        record(&conn, &sample_record("第一条", "回复1")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        record(&conn, &sample_record("第二条", "回复2")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        record(&conn, &sample_record("第三条", "回复3")).unwrap();

        let list = history_list(
            &conn,
            &HistoryFilter {
                limit: 10,
                offset: 0,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(list.len(), 3);
        // 倒序：最新写入的（第三条）应在最前
        assert_eq!(list[0].origin, "第三条");
        assert_eq!(list[1].origin, "第二条");
        assert_eq!(list[2].origin, "第一条");
    }

    #[test]
    fn test_history_search_match_origin() {
        let conn = new_mem_db();
        record(&conn, &sample_record("你好世界", "嗨")).unwrap();
        record(&conn, &sample_record("再见", "拜拜")).unwrap();

        let list = history_list(
            &conn,
            &HistoryFilter {
                limit: 10,
                offset: 0,
                search: Some("你好".to_string()),
            },
        )
        .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].origin, "你好世界");
    }

    #[test]
    fn test_history_search_match_selected() {
        let conn = new_mem_db();
        record(&conn, &sample_record("A", "你好世界")).unwrap();
        record(&conn, &sample_record("B", "再见")).unwrap();

        let list = history_list(
            &conn,
            &HistoryFilter {
                limit: 10,
                offset: 0,
                search: Some("你好".to_string()),
            },
        )
        .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].selected, "你好世界");
    }

    #[test]
    fn test_history_search_case_insensitive() {
        let conn = new_mem_db();
        record(&conn, &sample_record("Hello World", "Hi")).unwrap();
        // 小写搜索应匹配大写原文
        let list = history_list(
            &conn,
            &HistoryFilter {
                limit: 10,
                offset: 0,
                search: Some("hello".to_string()),
            },
        )
        .unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_history_pagination() {
        let conn = new_mem_db();
        // 写入 5 条
        for i in 0..5 {
            record(&conn, &sample_record(&format!("o{i}"), &format!("s{i}"))).unwrap();
        }
        // 分页：limit=2, offset=0
        let page1 = history_list(
            &conn,
            &HistoryFilter {
                limit: 2,
                offset: 0,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(page1.len(), 2);

        // 分页：limit=2, offset=2
        let page2 = history_list(
            &conn,
            &HistoryFilter {
                limit: 2,
                offset: 2,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(page2.len(), 2);
        // 两页 ID 不重叠
        assert_ne!(page1[0].id, page2[0].id);

        // 分页：limit=2, offset=4（只剩 1 条）
        let page3 = history_list(
            &conn,
            &HistoryFilter {
                limit: 2,
                offset: 4,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(page3.len(), 1);
    }

    #[test]
    fn test_history_count_total() {
        let conn = new_mem_db();
        record(&conn, &sample_record("A", "a")).unwrap();
        record(&conn, &sample_record("B", "b")).unwrap();
        assert_eq!(history_count(&conn, None).unwrap(), 2);
    }

    #[test]
    fn test_history_count_with_search() {
        let conn = new_mem_db();
        record(&conn, &sample_record("你好", "嗨")).unwrap();
        record(&conn, &sample_record("再见", "拜")).unwrap();
        // 搜索命中 1 条
        assert_eq!(history_count(&conn, Some("你好")).unwrap(), 1);
        // 搜索无命中
        assert_eq!(history_count(&conn, Some("不存在")).unwrap(), 0);
    }

    #[test]
    fn test_history_delete_single() {
        let conn = new_mem_db();
        record(&conn, &sample_record("A", "a")).unwrap();
        record(&conn, &sample_record("B", "b")).unwrap();
        assert_eq!(history_count(&conn, None).unwrap(), 2);

        // 取第一条的 ID 删除
        let list = history_list(
            &conn,
            &HistoryFilter {
                limit: 10,
                offset: 0,
                search: None,
            },
        )
        .unwrap();
        let id = list[0].id.unwrap();
        history_delete(&conn, id).unwrap();
        assert_eq!(history_count(&conn, None).unwrap(), 1);
    }

    #[test]
    fn test_history_clear_empties_table() {
        let conn = new_mem_db();
        record(&conn, &sample_record("A", "a")).unwrap();
        record(&conn, &sample_record("B", "b")).unwrap();
        assert_eq!(history_count(&conn, None).unwrap(), 2);

        history_clear(&conn).unwrap();
        assert_eq!(history_count(&conn, None).unwrap(), 0);
    }

    // ===== 边界场景 =====

    #[test]
    fn test_history_list_empty_table() {
        let conn = new_mem_db();
        let list = history_list(
            &conn,
            &HistoryFilter {
                limit: 10,
                offset: 0,
                search: None,
            },
        )
        .unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_history_count_empty_table() {
        let conn = new_mem_db();
        assert_eq!(history_count(&conn, None).unwrap(), 0);
    }

    #[test]
    fn test_history_clear_empty_table() {
        let conn = new_mem_db();
        // 空表清空不应报错
        history_clear(&conn).unwrap();
        assert_eq!(history_count(&conn, None).unwrap(), 0);
    }

    #[test]
    fn test_history_limit_clamped() {
        let conn = new_mem_db();
        record(&conn, &sample_record("A", "a")).unwrap();
        // 超大 limit 应被钳制到 500，不报错
        let list = history_list(
            &conn,
            &HistoryFilter {
                limit: 99999,
                offset: 0,
                search: None,
            },
        )
        .unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_history_offset_beyond_end() {
        let conn = new_mem_db();
        record(&conn, &sample_record("A", "a")).unwrap();
        // offset 超出表大小应返回空列表（SQLite 不报错）
        let list = history_list(
            &conn,
            &HistoryFilter {
                limit: 10,
                offset: 100,
                search: None,
            },
        )
        .unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_history_search_empty_string_returns_all() {
        let conn = new_mem_db();
        record(&conn, &sample_record("A", "a")).unwrap();
        record(&conn, &sample_record("B", "b")).unwrap();
        // 空搜索字符串应视为无搜索条件，返回全部
        let list = history_list(
            &conn,
            &HistoryFilter {
                limit: 10,
                offset: 0,
                search: Some("".to_string()),
            },
        )
        .unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_history_delete_nonexistent_id_safe() {
        let conn = new_mem_db();
        // 删除不存在的 ID 不应报错（DELETE 0 行不算错误）
        history_delete(&conn, 99999).unwrap();
        assert_eq!(history_count(&conn, None).unwrap(), 0);
    }
}
