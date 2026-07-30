// TODO 人工审查点：1.SQL 注入防护(params!) 2.空值处理 3.UPSERT 语义 4.敏感字段加密存储
// NOTE settings 表 KV 读写：键值对存储所有可配置项
// FIXME 安全遗留：llm_api_key 当前明文存储。后续应用 Windows DPAPI（CryptProtectData）
//       在 set/get 时透明加解密，并对已有明文数据做一次性迁移。当前仅本机访问，风险可控。
use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::error::AppResult;

pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get::<_, String>(0)?)),
        None => Ok(None),
    }
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_all_settings(conn: &Connection) -> AppResult<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (k, v) = row?;
        map.insert(k, v);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    #[test]
    fn test_set_get_roundtrip() {
        let conn = init_db(&std::env::temp_dir().join("st_test_settings.db")).unwrap();
        set_setting(&conn, "k1", "v1").unwrap();
        assert_eq!(get_setting(&conn, "k1").unwrap(), Some("v1".into()));
    }

    #[test]
    fn test_get_missing_returns_none() {
        let conn = init_db(&std::env::temp_dir().join("st_test_settings2.db")).unwrap();
        assert_eq!(get_setting(&conn, "missing").unwrap(), None);
    }
}
