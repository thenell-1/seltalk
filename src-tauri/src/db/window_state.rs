// TODO 人工审查点：1.坐标边界 2.UPSERT 3.布尔存储 4.ts-rs 类型导出
// NOTE 窗口布局持久化：位置/尺寸/置顶状态，按窗口 label 存储
//       P4.4：WindowState 派生 TS，cargo test 时自动生成 .ts 到 ./bindings/db/
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../bindings/db/WindowState.ts")]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub always_on_top: bool,
}

pub fn window_state_save(conn: &Connection, label: &str, s: &WindowState) -> AppResult<()> {
    conn.execute(
        "INSERT INTO window_state (label, x, y, w, h, always_on_top) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(label) DO UPDATE SET x=excluded.x, y=excluded.y, w=excluded.w, h=excluded.h, always_on_top=excluded.always_on_top",
        params![label, s.x, s.y, s.w, s.h, s.always_on_top as i64],
    )?;
    Ok(())
}

pub fn window_state_load(conn: &Connection, label: &str) -> AppResult<Option<WindowState>> {
    let mut stmt =
        conn.prepare("SELECT x, y, w, h, always_on_top FROM window_state WHERE label = ?1")?;
    let mut rows = stmt.query(params![label])?;
    match rows.next()? {
        Some(row) => Ok(Some(WindowState {
            x: row.get(0)?,
            y: row.get(1)?,
            w: row.get(2)?,
            h: row.get(3)?,
            always_on_top: row.get::<_, i64>(4)? != 0,
        })),
        None => Ok(None),
    }
}
