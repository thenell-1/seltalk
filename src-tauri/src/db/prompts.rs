// TODO 人工审查点：1.默认模板唯一性约束 2.序列化字段对齐 3.时间戳一致 4.ts-rs 类型导出 5.tags 逗号分隔解析
// NOTE Prompt 模板 CRUD + 默认模板切换 + 标签查询
//       tags 列存储逗号分隔字符串（如 "简短,正式"），前端按逗号拆分还原数组
//       P4.4：PromptTemplate 派生 TS，cargo test 时自动生成 .ts 到 ./bindings/db/
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../bindings/db/PromptTemplate.ts")]
pub struct PromptTemplate {
    #[ts(type = "number | null")]
    pub id: Option<i64>,
    pub name: String,
    pub template: String,
    pub is_default: bool,
    /// 标签（逗号分隔字符串，如 "简短,正式"；空串表示无标签）
    pub tags: String,
    pub created_at: String,
    pub updated_at: String,
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

/// 将逗号分隔的 tags 字符串拆分为去重后的标签数组（用于 prompt_all_tags）
fn split_tags(tags: &str) -> Vec<String> {
    tags.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn prompt_list(conn: &Connection) -> AppResult<Vec<PromptTemplate>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, template, is_default, tags, created_at, updated_at FROM prompts ORDER BY is_default DESC, id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(PromptTemplate {
            id: Some(r.get(0)?),
            name: r.get(1)?,
            template: r.get(2)?,
            is_default: r.get::<_, i64>(3)? != 0,
            tags: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn prompt_create(conn: &Connection, name: &str, template: &str, tags: &str) -> AppResult<i64> {
    let ts = now();
    conn.execute(
        "INSERT INTO prompts (name, template, is_default, tags, created_at, updated_at) VALUES (?1, ?2, 0, ?3, ?4, ?4)",
        params![name, template, tags, ts],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn prompt_update(
    conn: &Connection,
    id: i64,
    name: &str,
    template: &str,
    tags: &str,
) -> AppResult<()> {
    conn.execute(
        "UPDATE prompts SET name = ?1, template = ?2, tags = ?3, updated_at = ?4 WHERE id = ?5",
        params![name, template, tags, now(), id],
    )?;
    Ok(())
}

pub fn prompt_delete(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM prompts WHERE id = ?1", params![id])?;
    Ok(())
}

/// 设某模板为默认，其余全部取消默认
pub fn prompt_set_default(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("UPDATE prompts SET is_default = 0 WHERE is_default = 1", [])?;
    conn.execute(
        "UPDATE prompts SET is_default = 1, updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

pub fn prompt_get_default(conn: &Connection) -> AppResult<Option<PromptTemplate>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, template, is_default, tags, created_at, updated_at FROM prompts WHERE is_default = 1 LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], |r| {
        Ok(PromptTemplate {
            id: Some(r.get(0)?),
            name: r.get(1)?,
            template: r.get(2)?,
            is_default: r.get::<_, i64>(3)? != 0,
            tags: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
        })
    })?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

/// 查询全库去重后的标签列表（供前端标签自动补全）
///
/// 合并所有模板的 tags 字段，按逗号拆分后去重，按字母序排序
pub fn prompt_all_tags(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT tags FROM prompts WHERE tags != ''")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut set = std::collections::BTreeSet::new();
    for row in rows {
        let tags: String = row?;
        for t in split_tags(&tags) {
            set.insert(t);
        }
    }
    Ok(set.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;

    fn new_mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn
    }

    #[test]
    fn test_prompt_create_with_tags() {
        let conn = new_mem_db();
        let id = prompt_create(&conn, "简短模板", "回复：{{origin}}", "简短,口语").unwrap();
        let list = prompt_list(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, Some(id));
        assert_eq!(list[0].tags, "简短,口语");
    }

    #[test]
    fn test_prompt_update_tags() {
        let conn = new_mem_db();
        let id = prompt_create(&conn, "模板", "内容", "简短").unwrap();
        prompt_update(&conn, id, "模板", "新内容", "正式,委婉").unwrap();
        let list = prompt_list(&conn).unwrap();
        assert_eq!(list[0].tags, "正式,委婉");
        assert_eq!(list[0].template, "新内容");
    }

    #[test]
    fn test_prompt_empty_tags_default() {
        // 不传 tags 时应为空串（DEFAULT '' 兜底，但 prompt_create 显式传空串）
        let conn = new_mem_db();
        prompt_create(&conn, "无标签模板", "内容", "").unwrap();
        let list = prompt_list(&conn).unwrap();
        assert_eq!(list[0].tags, "");
    }

    #[test]
    fn test_prompt_all_tags_dedup() {
        let conn = new_mem_db();
        prompt_create(&conn, "t1", "c", "简短,正式").unwrap();
        prompt_create(&conn, "t2", "c", "正式,幽默").unwrap();
        prompt_create(&conn, "t3", "c", "").unwrap(); // 空标签不计入
        let tags = prompt_all_tags(&conn).unwrap();
        // BTreeSet 排序：幽默/正式/简短（按 Unicode 序）
        assert_eq!(tags, vec!["幽默", "正式", "简短"]);
    }

    #[test]
    fn test_prompt_all_tags_empty_when_no_tags() {
        let conn = new_mem_db();
        prompt_create(&conn, "t1", "c", "").unwrap();
        let tags = prompt_all_tags(&conn).unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn test_split_tags_trims_whitespace() {
        // 标签前后空格应被 trim
        let tags = split_tags(" 简短 , 正式 , ");
        assert_eq!(tags, vec!["简短", "正式"]);
    }
}
