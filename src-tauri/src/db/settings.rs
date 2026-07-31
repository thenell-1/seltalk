// TODO 人工审查点：1.SQL 注入防护(params!) 2.空值处理 3.UPSERT 语义 4.敏感字段加密存储 5.明文迁移
// NOTE settings 表 KV 读写：键值对存储所有可配置项
// 敏感字段（llm_api_key）通过 Windows DPAPI 透明加解密，明文不落库
// 迁移策略：读到无 dpapi: 前缀的明文 → 返回原值 + 日志告警，下次 set 时自动加密
use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::error::AppResult;

/// 需要加密存储的敏感 key 列表
const SENSITIVE_KEYS: &[&str] = &["llm_api_key"];

/// 判断 key 是否为敏感字段（需加密）
fn is_sensitive(key: &str) -> bool {
    SENSITIVE_KEYS.contains(&key)
}

/// 读取时：敏感字段透明解密
///
/// 返回值：
/// - Ok(value)：解密后的明文（或非敏感字段原值）
/// - Err(...)：解密失败（数据损坏或非本用户加密）
#[cfg(target_os = "windows")]
fn decrypt_value(key: &str, value: String) -> AppResult<String> {
    // 非敏感字段：直接返回原值
    if !is_sensitive(key) {
        return Ok(value);
    }
    // 敏感字段：通过 is_encrypted 判断格式，决定解密路径
    if crate::security::is_encrypted(&value) {
        // 加密格式：调用 DPAPI 解密为明文
        match crate::security::decrypt(&value)? {
            Some(plaintext) => Ok(plaintext),
            None => Ok(value), // 理论不可达（is_encrypted=true 但 decrypt 返回 None）
        }
    } else {
        // 明文（迁移）：返回原值，下次 set 时自动加密
        tracing::info!("检测到明文 {key}，将在下次 set 时自动加密");
        Ok(value)
    }
}

/// 非 Windows：不加密，直接返回原值
#[cfg(not(target_os = "windows"))]
fn decrypt_value(_key: &str, value: String) -> AppResult<String> {
    Ok(value)
}

/// 写入时：敏感字段透明加密
#[cfg(target_os = "windows")]
fn encrypt_value(key: &str, value: &str) -> AppResult<String> {
    if is_sensitive(key) {
        crate::security::encrypt(value)
    } else {
        Ok(value.to_string())
    }
}

/// 非 Windows：不加密，直接返回原值
#[cfg(not(target_os = "windows"))]
fn encrypt_value(_key: &str, value: &str) -> AppResult<String> {
    Ok(value.to_string())
}

pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    match rows.next()? {
        Some(row) => {
            let value: String = row.get(0)?;
            Ok(Some(decrypt_value(key, value)?))
        }
        None => Ok(None),
    }
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    let stored_value = encrypt_value(key, value)?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, stored_value],
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
        let v = decrypt_value(&k, v)?;
        map.insert(k, v);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;
    use rusqlite::Connection;

    /// 构造干净的内存数据库（避免文件残留导致的 UNIQUE 约束冲突）
    fn new_mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn
    }

    #[test]
    fn test_set_get_roundtrip() {
        let conn = new_mem_db();
        set_setting(&conn, "k1", "v1").unwrap();
        assert_eq!(get_setting(&conn, "k1").unwrap(), Some("v1".into()));
    }

    #[test]
    fn test_get_missing_returns_none() {
        let conn = new_mem_db();
        assert_eq!(get_setting(&conn, "missing").unwrap(), None);
    }

    #[test]
    fn test_api_key_encrypted_in_db() {
        // 敏感字段（llm_api_key）：set 后 DB 中应存储加密值，get 返回明文
        let conn = new_mem_db();
        let plaintext = "sk-test-key-12345";

        set_setting(&conn, "llm_api_key", plaintext).unwrap();

        // get_setting 应返回明文
        assert_eq!(
            get_setting(&conn, "llm_api_key").unwrap(),
            Some(plaintext.into())
        );

        // DB 中存储的值不应等于明文（应加密）
        let stored: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'llm_api_key'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(stored, plaintext, "API Key 不应明文存储");
        assert!(
            stored.starts_with(crate::security::DPAPI_PREFIX),
            "应以 dpapi: 前缀存储"
        );
    }

    #[test]
    fn test_non_sensitive_key_not_encrypted() {
        // 非敏感字段：set 后 DB 中存储明文
        let conn = new_mem_db();
        set_setting(&conn, "llm_model", "gpt-4").unwrap();

        let stored: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'llm_model'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, "gpt-4", "非敏感字段应明文存储");
    }

    #[test]
    fn test_plaintext_migration() {
        // 明文迁移：直接插入明文 → get_setting 应返回原值（不崩溃）
        let conn = new_mem_db();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('llm_api_key', 'sk-plaintext-old')",
            [],
        )
        .unwrap();

        // get_setting 应返回明文（迁移模式）
        let result = get_setting(&conn, "llm_api_key").unwrap();
        assert_eq!(result, Some("sk-plaintext-old".into()));
    }
}
