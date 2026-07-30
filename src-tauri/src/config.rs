// NOTE 应用配置存储模块
// 配置文件位于 %APPDATA%\CreativeInputMethod\config.json

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub trigger_key: String,
    pub candidate_count: u32,
    pub typing_speed: u32,
    pub typing_delay_min: u32,
    pub typing_delay_max: u32,
    pub llm_mode: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            trigger_key: "F8".to_string(),
            candidate_count: 3,
            typing_speed: 5,
            typing_delay_min: 50,
            typing_delay_max: 150,
            llm_mode: "cloud".to_string(),
        }
    }
}

/// LLM 模型配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub cloud_api_key: String,
    pub cloud_endpoint: String,
    pub cloud_model: String,
    pub local_endpoint: String,
    pub local_model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            cloud_api_key: String::new(),
            cloud_endpoint: "https://api.deepseek.com/v1".to_string(),
            cloud_model: "deepseek-chat".to_string(),
            local_endpoint: "http://localhost:11434".to_string(),
            local_model: "qwen2.5:7b".to_string(),
        }
    }
}

/// 获取配置目录
pub fn config_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Config(format!("获取配置目录失败: {e}")))?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// 读取应用配置
pub fn load_app_config(app: &AppHandle) -> AppResult<AppConfig> {
    let dir = config_dir(app)?;
    let path = dir.join("config.json");
    if !path.exists() {
        let default_config = AppConfig::default();
        save_app_config(app, &default_config)?;
        return Ok(default_config);
    }
    let content = std::fs::read_to_string(&path)?;
    let config: AppConfig = serde_json::from_str(&content)?;
    Ok(config)
}

/// 保存应用配置（自动 trim 字符串字段）
pub fn save_app_config(app: &AppHandle, config: &AppConfig) -> AppResult<()> {
    let dir = config_dir(app)?;
    let path = dir.join("config.json");
    let trimmed = AppConfig {
        trigger_key: config.trigger_key.trim().to_string(),
        llm_mode: config.llm_mode.trim().to_string(),
        candidate_count: config.candidate_count,
        typing_speed: config.typing_speed,
        typing_delay_min: config.typing_delay_min,
        typing_delay_max: config.typing_delay_max,
    };
    let content = serde_json::to_string_pretty(&trimmed)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// 读取 LLM 配置
pub fn load_llm_config(app: &AppHandle) -> AppResult<LlmConfig> {
    let dir = config_dir(app)?;
    let path = dir.join("llm_config.json");
    if !path.exists() {
        let default_config = LlmConfig::default();
        save_llm_config(app, &default_config)?;
        return Ok(default_config);
    }
    let content = std::fs::read_to_string(&path)?;
    let config: LlmConfig = serde_json::from_str(&content)?;
    Ok(config)
}

/// 保存 LLM 配置（自动 trim 字符串字段）
pub fn save_llm_config(app: &AppHandle, config: &LlmConfig) -> AppResult<()> {
    let dir = config_dir(app)?;
    let path = dir.join("llm_config.json");
    let trimmed = LlmConfig {
        cloud_api_key: config.cloud_api_key.trim().to_string(),
        cloud_endpoint: config.cloud_endpoint.trim().to_string(),
        cloud_model: config.cloud_model.trim().to_string(),
        local_endpoint: config.local_endpoint.trim().to_string(),
        local_model: config.local_model.trim().to_string(),
    };
    let content = serde_json::to_string_pretty(&trimmed)?;
    std::fs::write(&path, content)?;
    Ok(())
}
