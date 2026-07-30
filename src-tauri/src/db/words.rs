// TODO 人工审查点：1.SQL 注入防护(params!) 2.批量导入事务 3.去重逻辑 4.搜索 LIKE 转义
// NOTE 词库 CRUD + 批量导入/导出 + 分类/启禁用/搜索
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// 词库条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordEntry {
    pub id: Option<i64>,
    pub word: String,
    pub category: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 批量导入结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    pub imported: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
}

/// 列表筛选参数
#[derive(Debug, Clone, Default)]
pub struct WordFilter {
    /// 搜索关键字（模糊匹配 word 字段）
    pub search: Option<String>,
    /// 分类筛选
    pub category: Option<String>,
    /// 仅启用项
    pub enabled_only: bool,
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

/// 查询词库列表（支持搜索/分类/启禁用筛选）
pub fn word_list(conn: &Connection, filter: &WordFilter) -> AppResult<Vec<WordEntry>> {
    let mut sql = String::from(
        "SELECT id, word, category, enabled, created_at, updated_at FROM words WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ref search) = filter.search {
        sql.push_str(" AND word LIKE ? COLLATE NOCASE");
        param_values.push(Box::new(format!("%{search}%")));
    }
    if let Some(ref cat) = filter.category {
        if !cat.is_empty() {
            sql.push_str(" AND category = ?");
            param_values.push(Box::new(cat.clone()));
        }
    }
    if filter.enabled_only {
        sql.push_str(" AND enabled = 1");
    }
    sql.push_str(" ORDER BY category ASC, id DESC");

    let params_ref: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_ref.as_slice(), |r| {
        Ok(WordEntry {
            id: Some(r.get(0)?),
            word: r.get(1)?,
            category: r.get(2)?,
            enabled: r.get::<_, i64>(3)? != 0,
            created_at: r.get(4)?,
            updated_at: r.get(5)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 新增词条
pub fn word_create(conn: &Connection, word: &str, category: &str) -> AppResult<i64> {
    let ts = now();
    conn.execute(
        "INSERT INTO words (word, category, enabled, created_at, updated_at) VALUES (?1, ?2, 1, ?3, ?3)",
        params![word.trim(), category.trim(), ts],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 更新词条
pub fn word_update(conn: &Connection, id: i64, word: &str, category: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE words SET word = ?1, category = ?2, updated_at = ?3 WHERE id = ?4",
        params![word.trim(), category.trim(), now(), id],
    )?;
    Ok(())
}

/// 删除词条
pub fn word_delete(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM words WHERE id = ?1", params![id])?;
    Ok(())
}

/// 切换启禁用
pub fn word_toggle_enable(conn: &Connection, id: i64, enabled: bool) -> AppResult<()> {
    conn.execute(
        "UPDATE words SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
        params![enabled as i64, now(), id],
    )?;
    Ok(())
}

/// 批量导入（事务内执行，重复词跳过）
pub fn word_batch_import(
    conn: &Connection,
    entries: &[(String, String)],
) -> AppResult<BatchResult> {
    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = Vec::new();
    let ts = now();

    let tx = conn.unchecked_transaction()?;
    for (i, (word, category)) in entries.iter().enumerate() {
        let word = word.trim();
        if word.is_empty() {
            skipped += 1;
            continue;
        }
        // 查重（同 word + category 视为重复）
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM words WHERE word = ?1 AND category = ?2",
            params![word, category.trim()],
            |r| r.get(0),
        )?;
        if exists > 0 {
            skipped += 1;
            continue;
        }
        match tx.execute(
            "INSERT INTO words (word, category, enabled, created_at, updated_at) VALUES (?1, ?2, 1, ?3, ?3)",
            params![word, category.trim(), ts],
        ) {
            Ok(_) => imported += 1,
            Err(e) => {
                errors.push(format!("第 {} 行「{}」导入失败: {}", i + 1, word, e));
            }
        }
    }
    tx.commit()?;

    tracing::info!(
        "批量导入完成: 成功 {}, 跳过 {}, 错误 {}",
        imported,
        skipped,
        errors.len()
    );
    Ok(BatchResult {
        imported,
        skipped,
        errors,
    })
}

/// 导出全部词库为 JSON 字符串
pub fn word_export_json(conn: &Connection) -> AppResult<String> {
    let all = word_list(conn, &WordFilter::default())?;
    serde_json::to_string_pretty(&all).map_err(crate::error::AppError::Serde)
}

/// 获取全部启用词条（用于 LLM Prompt 注入 {{words}} 变量）
pub fn word_get_enabled(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT word FROM words WHERE enabled = 1 ORDER BY category ASC, id ASC")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 获取全部分类列表
pub fn word_categories(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT category FROM words WHERE category != '' ORDER BY category ASC",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    fn test_db() -> Connection {
        let path = std::env::temp_dir().join(format!(
            "st_test_words_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        init_db(&path).unwrap()
    }

    #[test]
    fn test_create_and_list() {
        let conn = test_db();
        word_create(&conn, "你好", "问候").unwrap();
        word_create(&conn, "在的", "问候").unwrap();
        let list = word_list(&conn, &WordFilter::default()).unwrap();
        assert_eq!(list.len(), 2);
        // 同分类按 id DESC（最新在前）：在的 后创建，排在前面
        assert_eq!(list[0].word, "在的");
        assert!(list[0].enabled);
    }

    #[test]
    fn test_search_filter() {
        let conn = test_db();
        word_create(&conn, "你好", "问候").unwrap();
        word_create(&conn, "再见", "告别").unwrap();
        let filter = WordFilter {
            search: Some("你".into()),
            ..Default::default()
        };
        let list = word_list(&conn, &filter).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].word, "你好");
    }

    #[test]
    fn test_category_filter() {
        let conn = test_db();
        word_create(&conn, "你好", "问候").unwrap();
        word_create(&conn, "再见", "告别").unwrap();
        let filter = WordFilter {
            category: Some("问候".into()),
            ..Default::default()
        };
        let list = word_list(&conn, &filter).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_toggle_enable() {
        let conn = test_db();
        let id = word_create(&conn, "测试", "").unwrap();
        word_toggle_enable(&conn, id, false).unwrap();
        let filter = WordFilter {
            enabled_only: true,
            ..Default::default()
        };
        let list = word_list(&conn, &filter).unwrap();
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_batch_import_no_dup() {
        let conn = test_db();
        word_create(&conn, "你好", "问候").unwrap();
        let entries = vec![
            ("你好".into(), "问候".into()), // 重复，应跳过
            ("在的".into(), "问候".into()),
            ("好的".into(), "通用".into()),
        ];
        let result = word_batch_import(&conn, &entries).unwrap();
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn test_export_json() {
        let conn = test_db();
        word_create(&conn, "你好", "问候").unwrap();
        let json = word_export_json(&conn).unwrap();
        assert!(json.contains("你好"));
        assert!(json.contains("问候"));
    }

    #[test]
    fn test_get_enabled() {
        let conn = test_db();
        word_create(&conn, "你好", "").unwrap();
        let id = word_create(&conn, "测试", "").unwrap();
        word_toggle_enable(&conn, id, false).unwrap();
        let enabled = word_get_enabled(&conn).unwrap();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0], "你好");
    }

    #[test]
    fn test_delete() {
        let conn = test_db();
        let id = word_create(&conn, "测试", "").unwrap();
        word_delete(&conn, id).unwrap();
        let list = word_list(&conn, &WordFilter::default()).unwrap();
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_categories() {
        let conn = test_db();
        word_create(&conn, "你好", "问候").unwrap();
        word_create(&conn, "再见", "告别").unwrap();
        word_create(&conn, "好的", "问候").unwrap();
        let cats = word_categories(&conn).unwrap();
        assert_eq!(cats.len(), 2);
        assert!(cats.contains(&"问候".to_string()));
        assert!(cats.contains(&"告别".to_string()));
    }
}
