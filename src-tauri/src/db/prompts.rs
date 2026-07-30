// TODO 人工审查点：1.默认模板唯一性约束 2.序列化字段对齐 3.时间戳一致
// NOTE Prompt 模板 CRUD + 默认模板切换
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub id: Option<i64>,
    pub name: String,
    pub template: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

pub fn prompt_list(conn: &Connection) -> AppResult<Vec<PromptTemplate>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, template, is_default, created_at, updated_at FROM prompts ORDER BY is_default DESC, id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(PromptTemplate {
            id: Some(r.get(0)?),
            name: r.get(1)?,
            template: r.get(2)?,
            is_default: r.get::<_, i64>(3)? != 0,
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

pub fn prompt_create(conn: &Connection, name: &str, template: &str) -> AppResult<i64> {
    let ts = now();
    conn.execute(
        "INSERT INTO prompts (name, template, is_default, created_at, updated_at) VALUES (?1, ?2, 0, ?3, ?3)",
        params![name, template, ts],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn prompt_update(conn: &Connection, id: i64, name: &str, template: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE prompts SET name = ?1, template = ?2, updated_at = ?3 WHERE id = ?4",
        params![name, template, now(), id],
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
        "SELECT id, name, template, is_default, created_at, updated_at FROM prompts WHERE is_default = 1 LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], |r| {
        Ok(PromptTemplate {
            id: Some(r.get(0)?),
            name: r.get(1)?,
            template: r.get(2)?,
            is_default: r.get::<_, i64>(3)? != 0,
            created_at: r.get(4)?,
            updated_at: r.get(5)?,
        })
    })?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}
