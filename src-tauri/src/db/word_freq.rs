// TODO 人工审查点：1.UPSERT 语义正确性 2.事务完整性 3.SQL 注入防护 4.并发安全
// NOTE 词频表操作：记录选中候选的分词、查询高频词、重置词频
//       场景：用户在悬浮窗选中候选 → orchestrator 分词 → record_batch 累计 → 前端词云页 top 查询
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::AppResult;

/// 词频条目（对应 word_freq 表一行）
#[derive(Debug, Clone, Serialize)]
pub struct WordFreqEntry {
    /// 词语
    pub word: String,
    /// 使用次数
    pub count: i64,
    /// 最后使用时间（RFC3339 格式）
    pub last_used_at: Option<String>,
}

/// 记录单个词语的使用（UPSERT：存在则 count+1，不存在则插入 count=1）
///
/// # 参数
/// - `conn`：数据库连接
/// - `word`：待记录的词语
#[allow(dead_code)]
pub fn record(conn: &Connection, word: &str) -> AppResult<()> {
    let now = chrono::Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO word_freq (word, count, last_used_at)
         VALUES (?1, 1, ?2)
         ON CONFLICT(word) DO UPDATE SET
            count = count + 1,
            last_used_at = excluded.last_used_at",
        params![word, now],
    )?;
    Ok(())
}

/// 批量记录多个词语的使用（事务内逐个 UPSERT）
///
/// # 参数
/// - `conn`：数据库连接
/// - `words`：词语列表
pub fn record_batch(conn: &Connection, words: &[String]) -> AppResult<()> {
    if words.is_empty() {
        return Ok(());
    }
    let now = chrono::Local::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO word_freq (word, count, last_used_at)
             VALUES (?1, 1, ?2)
             ON CONFLICT(word) DO UPDATE SET
                count = count + 1,
                last_used_at = excluded.last_used_at",
        )?;
        for word in words {
            stmt.execute(params![word, now])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// 查询高频词列表（按 count 降序，取前 limit 条）
///
/// # 参数
/// - `conn`：数据库连接
/// - `limit`：最大返回条数（建议 50-200）
///
/// # 返回
/// 按词频降序排列的词频条目列表
pub fn top(conn: &Connection, limit: u32) -> AppResult<Vec<WordFreqEntry>> {
    let limit = limit.min(500) as i64; // 上限保护，防止过大查询
    let mut stmt = conn.prepare(
        "SELECT word, count, last_used_at
         FROM word_freq
         ORDER BY count DESC, word ASC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok(WordFreqEntry {
            word: r.get::<_, String>(0)?,
            count: r.get::<_, i64>(1)?,
            last_used_at: r.get::<_, Option<String>>(2)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// 重置词频表（清空全部记录）
///
/// # 参数
/// - `conn`：数据库连接
pub fn reset(conn: &Connection) -> AppResult<()> {
    conn.execute("DELETE FROM word_freq", [])?;
    tracing::info!("词频表已重置");
    Ok(())
}

/// 获取词频表中的词语总数
pub fn count_total(conn: &Connection) -> AppResult<i64> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM word_freq", [], |r| r.get(0))?;
    Ok(count)
}

/// 获取词频累计总次数（所有词语的 count 之和）
pub fn count_total_usage(conn: &Connection) -> AppResult<i64> {
    let count: i64 =
        conn.query_row("SELECT COALESCE(SUM(count), 0) FROM word_freq", [], |r| r.get(0))?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    /// 辅助：创建内存数据库
    fn test_db() -> Connection {
        let path = std::env::temp_dir().join(format!(
            "st_test_wordfreq_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        init_db(&path).unwrap()
    }

    // ===== 正常流程测试 =====

    #[test]
    fn test_record_single_word() {
        let conn = test_db();
        record(&conn, "你好").unwrap();
        let entries = top(&conn, 10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].word, "你好");
        assert_eq!(entries[0].count, 1);
    }

    #[test]
    fn test_record_increments_count() {
        let conn = test_db();
        record(&conn, "你好").unwrap();
        record(&conn, "你好").unwrap();
        record(&conn, "你好").unwrap();
        let entries = top(&conn, 10).unwrap();
        assert_eq!(entries[0].count, 3);
    }

    #[test]
    fn test_record_batch() {
        let conn = test_db();
        let words = vec!["你好".to_string(), "世界".to_string(), "朋友".to_string()];
        record_batch(&conn, &words).unwrap();
        let entries = top(&conn, 10).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_record_batch_increment() {
        let conn = test_db();
        let words1 = vec!["你好".to_string(), "世界".to_string()];
        record_batch(&conn, &words1).unwrap();
        // 第二次批量记录，"你好" 应 +1
        let words2 = vec!["你好".to_string(), "朋友".to_string()];
        record_batch(&conn, &words2).unwrap();
        let entries = top(&conn, 10).unwrap();
        let hello = entries.iter().find(|e| e.word == "你好").unwrap();
        assert_eq!(hello.count, 2);
        let world = entries.iter().find(|e| e.word == "世界").unwrap();
        assert_eq!(world.count, 1);
    }

    #[test]
    fn test_top_sorted_descending() {
        let conn = test_db();
        record(&conn, "低频").unwrap();
        for _ in 0..5 {
            record(&conn, "高频").unwrap();
        }
        for _ in 0..3 {
            record(&conn, "中频").unwrap();
        }
        let entries = top(&conn, 10).unwrap();
        assert_eq!(entries[0].word, "高频");
        assert_eq!(entries[0].count, 5);
        assert_eq!(entries[1].word, "中频");
        assert_eq!(entries[1].count, 3);
        assert_eq!(entries[2].word, "低频");
        assert_eq!(entries[2].count, 1);
    }

    #[test]
    fn test_top_limit() {
        let conn = test_db();
        for i in 0..10 {
            record(&conn, &format!("词{i}")).unwrap();
        }
        let entries = top(&conn, 5).unwrap();
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn test_count_total() {
        let conn = test_db();
        record(&conn, "你好").unwrap();
        record(&conn, "世界").unwrap();
        assert_eq!(count_total(&conn).unwrap(), 2);
    }

    #[test]
    fn test_count_total_usage() {
        let conn = test_db();
        record(&conn, "你好").unwrap();
        record(&conn, "你好").unwrap();
        record(&conn, "世界").unwrap();
        assert_eq!(count_total_usage(&conn).unwrap(), 3);
    }

    // ===== 边界场景测试 =====

    #[test]
    fn test_top_empty_table() {
        let conn = test_db();
        let entries = top(&conn, 10).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_record_batch_empty() {
        let conn = test_db();
        let words: Vec<String> = vec![];
        record_batch(&conn, &words).unwrap();
        assert_eq!(count_total(&conn).unwrap(), 0);
    }

    #[test]
    fn test_reset_clears_all() {
        let conn = test_db();
        record(&conn, "你好").unwrap();
        record(&conn, "世界").unwrap();
        assert_eq!(count_total(&conn).unwrap(), 2);
        reset(&conn).unwrap();
        assert_eq!(count_total(&conn).unwrap(), 0);
    }

    #[test]
    fn test_reset_empty_table() {
        let conn = test_db();
        // 空表重置不应报错
        reset(&conn).unwrap();
        assert_eq!(count_total(&conn).unwrap(), 0);
    }

    #[test]
    fn test_top_limit_clamped() {
        let conn = test_db();
        record(&conn, "测试").unwrap();
        // 超大 limit 应被钳制到 500，不报错
        let entries = top(&conn, 99999).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_count_total_usage_empty() {
        let conn = test_db();
        assert_eq!(count_total_usage(&conn).unwrap(), 0);
    }

    // ===== 错误场景测试 =====

    #[test]
    fn test_record_same_word_repeated() {
        // 同一词重复记录不应出错
        let conn = test_db();
        for _ in 0..100 {
            record(&conn, "重复词").unwrap();
        }
        let entries = top(&conn, 10).unwrap();
        assert_eq!(entries[0].count, 100);
    }

    #[test]
    fn test_record_batch_with_duplicates() {
        // 批量记录含重复词，应逐个 UPSERT
        let conn = test_db();
        let words = vec!["你好".to_string(), "你好".to_string(), "你好".to_string()];
        record_batch(&conn, &words).unwrap();
        let entries = top(&conn, 10).unwrap();
        assert_eq!(entries[0].count, 3);
    }
}
