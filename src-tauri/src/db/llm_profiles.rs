// TODO 人工审查点：1.SQL 注入防护(params!) 2.api_key 加密一致性 3.is_active 互斥 4.空值处理 5.迁移幂等
// NOTE llm_profiles 表 CRUD + active 切换：多份命名 LLM 配置，is_active 标记当前生效配置
//       api_key 列通过 Windows DPAPI 透明加解密（与 db/settings.rs 一致），明文不落库
//       迁移策略：读到无 dpapi: 前缀的明文 → 返回原值 + 日志告警，下次写入时自动加密
//       P4.4：LlmProfile / LlmProfileInput 派生 TS，cargo test 时自动生成 .ts 到 ./bindings/db/
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, AppResult};

/// 单条 LLM 配置记录（多配置持久化）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../bindings/db/LlmProfile.ts")]
pub struct LlmProfile {
    #[ts(type = "number | null")]
    pub id: Option<i64>,
    pub name: String,
    pub base_url: String,
    /// 明文（内存中）；落库时加密，读取时解密
    pub api_key: String,
    pub model: String,
    pub model_type: String,
    pub temperature: f64,
    #[ts(type = "number")]
    pub max_tokens: u32,
    #[ts(type = "number")]
    pub max_context_length: u32,
    pub stream_enabled: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 创建/更新 LLM 配置时的输入（不含 id、is_active、时间戳）
///
/// 前端表单直接序列化为该结构传入 Tauri 命令
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../bindings/db/LlmProfileInput.ts")]
pub struct LlmProfileInput {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub model_type: String,
    pub temperature: f64,
    #[ts(type = "number")]
    pub max_tokens: u32,
    #[ts(type = "number")]
    pub max_context_length: u32,
    pub stream_enabled: bool,
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

// ===== DPAPI 加解密（与 db/settings.rs 一致：仅 Windows 加密，其他平台明文）=====

#[cfg(target_os = "windows")]
fn encrypt_api_key(plain: &str) -> AppResult<String> {
    crate::security::encrypt(plain)
}

#[cfg(not(target_os = "windows"))]
fn encrypt_api_key(plain: &str) -> AppResult<String> {
    Ok(plain.to_string())
}

#[cfg(target_os = "windows")]
fn decrypt_api_key(stored: &str) -> AppResult<String> {
    if crate::security::is_encrypted(stored) {
        match crate::security::decrypt(stored)? {
            Some(plain) => Ok(plain),
            None => Ok(stored.to_string()), // 理论不可达（is_encrypted=true 但 decrypt 返回 None）
        }
    } else {
        // 明文迁移：返回原值，下次写入时自动加密
        tracing::info!("检测到明文 llm_profiles.api_key，将在下次写入时自动加密");
        Ok(stored.to_string())
    }
}

#[cfg(not(target_os = "windows"))]
fn decrypt_api_key(stored: &str) -> AppResult<String> {
    Ok(stored.to_string())
}

// ===== 查询列模板（保证 list/get_active 列顺序一致，避免索引漂移）=====

const PROFILE_COLUMNS: &str = "id, name, base_url, api_key, model, model_type, temperature, \
     max_tokens, max_context_length, stream_enabled, is_active, created_at, updated_at";

/// 数据库原始行（api_key 为加密存储值，未解密）
struct LlmProfileRow {
    id: i64,
    name: String,
    base_url: String,
    api_key_stored: String,
    model: String,
    model_type: String,
    temperature: f64,
    max_tokens: i64,
    max_context_length: i64,
    stream_enabled: i64,
    is_active: i64,
    created_at: String,
    updated_at: String,
}

impl LlmProfileRow {
    fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            name: r.get(1)?,
            base_url: r.get(2)?,
            api_key_stored: r.get(3)?,
            model: r.get(4)?,
            model_type: r.get(5)?,
            temperature: r.get(6)?,
            max_tokens: r.get(7)?,
            max_context_length: r.get(8)?,
            stream_enabled: r.get(9)?,
            is_active: r.get(10)?,
            created_at: r.get(11)?,
            updated_at: r.get(12)?,
        })
    }

    /// 原始行 → LlmProfile（解密 api_key）
    fn into_profile(self) -> AppResult<LlmProfile> {
        Ok(LlmProfile {
            id: Some(self.id),
            name: self.name,
            base_url: self.base_url,
            api_key: decrypt_api_key(&self.api_key_stored)?,
            model: self.model,
            model_type: self.model_type,
            temperature: self.temperature,
            max_tokens: self.max_tokens as u32,
            max_context_length: self.max_context_length as u32,
            stream_enabled: self.stream_enabled != 0,
            is_active: self.is_active != 0,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

// ===== CRUD =====

/// 查询全部配置档案（active 优先，其次按更新时间倒序）
pub fn llm_profile_list(conn: &Connection) -> AppResult<Vec<LlmProfile>> {
    let sql = format!("SELECT {PROFILE_COLUMNS} FROM llm_profiles ORDER BY is_active DESC, updated_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], LlmProfileRow::from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?.into_profile()?);
    }
    Ok(out)
}

/// 查询当前生效配置（is_active=1）
pub fn llm_profile_get_active(conn: &Connection) -> AppResult<Option<LlmProfile>> {
    let sql = format!("SELECT {PROFILE_COLUMNS} FROM llm_profiles WHERE is_active = 1 LIMIT 1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map([], LlmProfileRow::from_row)?;
    match rows.next() {
        Some(r) => Ok(Some(r?.into_profile()?)),
        None => Ok(None),
    }
}

/// 新建配置档案并设为当前生效（先清空其他 active，再插入 is_active=1）
pub fn llm_profile_create(conn: &Connection, input: &LlmProfileInput) -> AppResult<i64> {
    let ts = now();
    let api_key_enc = encrypt_api_key(&input.api_key)?;
    // 新建即生效：保证同一时间仅一条 active
    conn.execute("UPDATE llm_profiles SET is_active = 0 WHERE is_active = 1", [])?;
    conn.execute(
        "INSERT INTO llm_profiles \
         (name, base_url, api_key, model, model_type, temperature, max_tokens, max_context_length, stream_enabled, is_active, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?10)",
        params![
            input.name,
            input.base_url,
            api_key_enc,
            input.model,
            input.model_type,
            input.temperature,
            input.max_tokens,
            input.max_context_length,
            input.stream_enabled as i64,
            ts,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 更新指定配置档案的字段（保留 is_active 状态不变）
pub fn llm_profile_update(conn: &Connection, id: i64, input: &LlmProfileInput) -> AppResult<()> {
    let api_key_enc = encrypt_api_key(&input.api_key)?;
    let affected = conn.execute(
        "UPDATE llm_profiles SET \
         name = ?1, base_url = ?2, api_key = ?3, model = ?4, model_type = ?5, \
         temperature = ?6, max_tokens = ?7, max_context_length = ?8, stream_enabled = ?9, \
         updated_at = ?10 \
         WHERE id = ?11",
        params![
            input.name,
            input.base_url,
            api_key_enc,
            input.model,
            input.model_type,
            input.temperature,
            input.max_tokens,
            input.max_context_length,
            input.stream_enabled as i64,
            now(),
            id,
        ],
    )?;
    if affected == 0 {
        return Err(AppError::Config(format!("LLM 配置 id={id} 不存在")));
    }
    Ok(())
}

/// 删除指定配置档案；若删除的是 active，则提升剩余最新一条为 active
pub fn llm_profile_delete(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM llm_profiles WHERE id = ?1", params![id])?;
    // 删除后若无 active，提升剩余 updated_at 最新的一条
    let active_exists: i64 =
        conn.query_row("SELECT COUNT(*) FROM llm_profiles WHERE is_active = 1", [], |r| {
            r.get(0)
        })?;
    if active_exists == 0 {
        let next_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM llm_profiles ORDER BY updated_at DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        if let Some(nid) = next_id {
            conn.execute(
                "UPDATE llm_profiles SET is_active = 1, updated_at = ?1 WHERE id = ?2",
                params![now(), nid],
            )?;
        }
    }
    Ok(())
}

/// 将指定配置档案设为当前生效（互斥：先全部置 0，再目标置 1）
pub fn llm_profile_set_active(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("UPDATE llm_profiles SET is_active = 0 WHERE is_active = 1", [])?;
    let affected = conn.execute(
        "UPDATE llm_profiles SET is_active = 1, updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    if affected == 0 {
        return Err(AppError::Config(format!("LLM 配置 id={id} 不存在")));
    }
    Ok(())
}

/// 一次性迁移：profiles 表为空 且 KV 存在 llm_model 时，从旧 KV 配置生成默认 active 配置
///
/// 幂等：表非空 或 KV 无 llm_model 时直接返回，不重复迁移。
/// 供 lib.rs setup 在 seed_if_empty 之后调用，保证老用户平滑升级。
pub fn ensure_default_profile(conn: &Connection) -> AppResult<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM llm_profiles", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(()); // 已有配置，不迁移
    }

    // KV 无 llm_model 视为未配置过，不迁移（用户在 UI 自行新建）
    let model = crate::db::settings::get_setting(conn, crate::config::KEY_LLM_MODEL)
        .ok()
        .flatten()
        .unwrap_or_default();
    if model.is_empty() {
        return Ok(());
    }

    // 从 KV 读取其余字段（缺失时回退默认值）
    let base_url = crate::db::settings::get_setting(conn, crate::config::KEY_LLM_BASE_URL)
        .ok()
        .flatten()
        .unwrap_or_default();
    // get_setting 已对 llm_api_key 透明解密，此处拿到明文
    let api_key = crate::db::settings::get_setting(conn, crate::config::KEY_LLM_API_KEY)
        .ok()
        .flatten()
        .unwrap_or_default();
    let temperature = crate::db::settings::get_setting(conn, crate::config::KEY_LLM_TEMPERATURE)
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(crate::config::DEFAULT_LLM_TEMPERATURE);
    let max_tokens = crate::db::settings::get_setting(conn, crate::config::KEY_LLM_MAX_TOKENS)
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(crate::config::DEFAULT_LLM_MAX_TOKENS);
    let stream_enabled = crate::db::settings::get_setting(conn, crate::config::KEY_LLM_STREAM_ENABLED)
        .ok()
        .flatten()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(crate::config::DEFAULT_LLM_STREAM_ENABLED);

    let input = LlmProfileInput {
        name: "默认配置".to_string(),
        base_url,
        api_key,
        model,
        model_type: crate::config::DEFAULT_LLM_MODEL_TYPE.to_string(),
        temperature,
        max_tokens,
        max_context_length: crate::config::DEFAULT_LLM_MAX_CONTEXT_LENGTH,
        stream_enabled,
    };
    llm_profile_create(conn, &input)?;
    tracing::info!("已从旧 KV 配置迁移出默认 LLM 配置档案");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;
    use rusqlite::Connection;

    /// 构造干净内存库（应用全部建表 SQL）
    fn new_mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn
    }

    fn sample_input(name: &str) -> LlmProfileInput {
        LlmProfileInput {
            name: name.to_string(),
            base_url: "https://api.openai.com".to_string(),
            api_key: "sk-test-key-12345".to_string(),
            model: "gpt-4o-mini".to_string(),
            model_type: "openai".to_string(),
            temperature: 0.6,
            max_tokens: 1024,
            max_context_length: 128000,
            stream_enabled: true,
        }
    }

    // ===== 正常流程 =====

    #[test]
    fn test_create_and_list() {
        let conn = new_mem_db();
        let id = llm_profile_create(&conn, &sample_input("A")).unwrap();
        assert!(id > 0);
        let list = llm_profile_list(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "A");
        assert_eq!(list[0].model, "gpt-4o-mini");
        assert!(list[0].is_active);
    }

    #[test]
    fn test_create_sets_active_mutual_exclusion() {
        // 新建即 active：第二条 create 后，第一条应变为非 active
        let conn = new_mem_db();
        llm_profile_create(&conn, &sample_input("A")).unwrap();
        llm_profile_create(&conn, &sample_input("B")).unwrap();
        let list = llm_profile_list(&conn).unwrap();
        assert_eq!(list.len(), 2);
        let active_count = list.iter().filter(|p| p.is_active).count();
        assert_eq!(active_count, 1, "同一时间仅一条 active");
        assert!(list.iter().any(|p| p.name == "B" && p.is_active));
    }

    #[test]
    fn test_get_active() {
        let conn = new_mem_db();
        llm_profile_create(&conn, &sample_input("A")).unwrap();
        llm_profile_create(&conn, &sample_input("B")).unwrap();
        let active = llm_profile_get_active(&conn).unwrap().unwrap();
        assert_eq!(active.name, "B");
    }

    #[test]
    fn test_get_active_none_when_empty() {
        let conn = new_mem_db();
        assert!(llm_profile_get_active(&conn).unwrap().is_none());
    }

    #[test]
    fn test_set_active_mutual_exclusion() {
        let conn = new_mem_db();
        let _ = llm_profile_create(&conn, &sample_input("A")).unwrap();
        let id_b = llm_profile_create(&conn, &sample_input("B")).unwrap();
        // 此时 B 为 active，切回 A... 先拿 A 的 id
        let list = llm_profile_list(&conn).unwrap();
        let id_a = list.iter().find(|p| p.name == "A").unwrap().id.unwrap();
        llm_profile_set_active(&conn, id_a).unwrap();
        let active = llm_profile_get_active(&conn).unwrap().unwrap();
        assert_eq!(active.id, Some(id_a));
        // B 应已变为非 active
        let b = llm_profile_list(&conn)
            .unwrap()
            .into_iter()
            .find(|p| p.id == Some(id_b))
            .unwrap();
        assert!(!b.is_active);
    }

    #[test]
    fn test_set_active_nonexistent_errors() {
        let conn = new_mem_db();
        let err = llm_profile_set_active(&conn, 9999).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
    }

    #[test]
    fn test_update_preserves_active_state() {
        let conn = new_mem_db();
        let id = llm_profile_create(&conn, &sample_input("A")).unwrap();
        let mut input = sample_input("A");
        input.model = "gpt-4o".to_string();
        llm_profile_update(&conn, id, &input).unwrap();
        let p = llm_profile_get_active(&conn).unwrap().unwrap();
        assert_eq!(p.model, "gpt-4o");
        assert!(p.is_active, "update 不改变 active 状态");
    }

    #[test]
    fn test_update_nonexistent_errors() {
        let conn = new_mem_db();
        let err = llm_profile_update(&conn, 9999, &sample_input("X")).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
    }

    #[test]
    fn test_delete_active_promotes_next() {
        let conn = new_mem_db();
        let _ = llm_profile_create(&conn, &sample_input("A")).unwrap();
        let id_b = llm_profile_create(&conn, &sample_input("B")).unwrap(); // B active
        llm_profile_delete(&conn, id_b).unwrap();
        let list = llm_profile_list(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].is_active, "删除 active 后剩余首条应被提升");
    }

    #[test]
    fn test_delete_last_leaves_empty() {
        let conn = new_mem_db();
        let id = llm_profile_create(&conn, &sample_input("A")).unwrap();
        llm_profile_delete(&conn, id).unwrap();
        assert!(llm_profile_list(&conn).unwrap().is_empty());
        assert!(llm_profile_get_active(&conn).unwrap().is_none());
    }

    // ===== api_key 加密 =====

    #[test]
    fn test_api_key_roundtrip() {
        // 写入加密值，读取得到明文
        let conn = new_mem_db();
        llm_profile_create(&conn, &sample_input("A")).unwrap();
        let p = llm_profile_get_active(&conn).unwrap().unwrap();
        assert_eq!(p.api_key, "sk-test-key-12345");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_api_key_encrypted_in_db() {
        // DB 中存储的 api_key 不应是明文（应带 dpapi: 前缀）
        let conn = new_mem_db();
        llm_profile_create(&conn, &sample_input("A")).unwrap();
        let stored: String = conn
            .query_row(
                "SELECT api_key FROM llm_profiles WHERE is_active = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(stored, "sk-test-key-12345", "不应明文存储");
        assert!(
            stored.starts_with(crate::security::DPAPI_PREFIX),
            "应以 dpapi: 前缀存储"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_api_key_update_reencrypts() {
        let conn = new_mem_db();
        let id = llm_profile_create(&conn, &sample_input("A")).unwrap();
        let mut input = sample_input("A");
        input.api_key = "sk-new-key-99999".to_string();
        llm_profile_update(&conn, id, &input).unwrap();
        let p = llm_profile_get_active(&conn).unwrap().unwrap();
        assert_eq!(p.api_key, "sk-new-key-99999");
        let stored: String = conn
            .query_row("SELECT api_key FROM llm_profiles WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(
            stored.starts_with(crate::security::DPAPI_PREFIX),
            "更新后仍应加密存储"
        );
    }

    // ===== 迁移幂等 =====

    #[test]
    fn test_ensure_default_profile_migrates_from_kv() {
        let conn = new_mem_db();
        // 模拟老 KV 配置
        crate::db::settings::set_setting(&conn, crate::config::KEY_LLM_MODEL, "gpt-4o").unwrap();
        crate::db::settings::set_setting(&conn, crate::config::KEY_LLM_BASE_URL, "https://api.openai.com").unwrap();
        crate::db::settings::set_setting(&conn, crate::config::KEY_LLM_API_KEY, "sk-legacy").unwrap();
        crate::db::settings::set_setting(&conn, crate::config::KEY_LLM_TEMPERATURE, "0.5").unwrap();

        ensure_default_profile(&conn).unwrap();
        let list = llm_profile_list(&conn).unwrap();
        assert_eq!(list.len(), 1, "应迁移出一条配置");
        assert!(list[0].is_active);
        assert_eq!(list[0].model, "gpt-4o");
        assert_eq!(list[0].api_key, "sk-legacy", "KV 中的 api_key 应透明解密迁移");
        assert_eq!(list[0].temperature, 0.5);
    }

    #[test]
    fn test_ensure_default_profile_idempotent() {
        let conn = new_mem_db();
        crate::db::settings::set_setting(&conn, crate::config::KEY_LLM_MODEL, "gpt-4o").unwrap();
        ensure_default_profile(&conn).unwrap();
        ensure_default_profile(&conn).unwrap(); // 二次调用不应再创建
        let list = llm_profile_list(&conn).unwrap();
        assert_eq!(list.len(), 1, "幂等：重复调用不重复迁移");
    }

    #[test]
    fn test_ensure_default_profile_no_kv_noop() {
        // KV 无 llm_model：不迁移
        let conn = new_mem_db();
        ensure_default_profile(&conn).unwrap();
        assert!(llm_profile_list(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_ensure_default_profile_nonempty_noop() {
        // 表已有配置：不迁移（即使 KV 有 llm_model）
        let conn = new_mem_db();
        llm_profile_create(&conn, &sample_input("A")).unwrap();
        crate::db::settings::set_setting(&conn, crate::config::KEY_LLM_MODEL, "gpt-4o").unwrap();
        ensure_default_profile(&conn).unwrap();
        assert_eq!(llm_profile_list(&conn).unwrap().len(), 1);
    }

    // ===== load_llm_config 集成（无 active 回退默认）=====

    #[test]
    fn test_load_llm_config_from_active_profile() {
        let conn = new_mem_db();
        llm_profile_create(&conn, &sample_input("A")).unwrap();
        let cfg = crate::llm::load_llm_config(&conn).unwrap();
        assert_eq!(cfg.model, "gpt-4o-mini");
        assert_eq!(cfg.model_type, "openai");
        assert_eq!(cfg.max_context_length, 128000);
    }

    #[test]
    fn test_load_llm_config_empty_falls_back_to_default() {
        let conn = new_mem_db();
        let cfg = crate::llm::load_llm_config(&conn).unwrap();
        assert_eq!(cfg.model, "");
        assert_eq!(cfg.model_type, crate::config::DEFAULT_LLM_MODEL_TYPE);
        assert_eq!(cfg.max_context_length, crate::config::DEFAULT_LLM_MAX_CONTEXT_LENGTH);
    }
}
