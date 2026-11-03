//! config.toml 读取：缺字段/缺文件/非法值回退默认值（PRD P0-07）。
//!
//! 运行时读取 `app_data_dir/config.toml`（首启由装配层从 resources 复制模板）；
//! 门禁状态在独立的 gate 状态文件中，删除 config.toml 不影响冷却（P0-06）。

use std::fs;
use std::path::Path;

use crate::error::AppError;
use crate::model::AppConfig;

/// 解析并校验配置文本；字段缺失由 serde 默认值兜底。
pub fn parse_config(text: &str) -> Result<AppConfig, AppError> {
    let mut cfg: AppConfig = toml::from_str(text)
        .map_err(|e| AppError::Config(format!("config.toml 解析失败：{}", e)))?;
    sanitize(&mut cfg);
    Ok(cfg)
}

/// 非法取值回退默认值（而非整个文件报错，保证家长改坏配置后程序仍可用）。
fn sanitize(cfg: &mut AppConfig) {
    if cfg.quiz.question_count == 0 {
        tracing::warn!("config.quiz.question_count 为 0，回退默认值 5");
        cfg.quiz.question_count = 5;
    }
    if !(cfg.quiz.pass_score > 0.0 && cfg.quiz.pass_score <= 1.0) {
        tracing::warn!("config.quiz.pass_score 越界，回退默认值 0.8");
        cfg.quiz.pass_score = 0.8;
    }
    if cfg.quiz.cool_down_sec == 0 {
        tracing::warn!("config.quiz.cool_down_sec 为 0，回退默认值 120");
        cfg.quiz.cool_down_sec = 120;
    }
    // 白名单 id 去重（重复条目只保留首个）
    let mut seen = std::collections::HashSet::new();
    cfg.target_apps.retain(|t| seen.insert(t.id.clone()));
}

/// 读取配置；缺文件/解析失败一律回退默认配置并记日志（PRD P0-07）。
pub fn load_or_default(path: &Path) -> AppConfig {
    match fs::read_to_string(path) {
        Ok(text) => match parse_config(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!(error = %e, "配置解析失败，使用默认配置");
                AppConfig::default()
            }
        },
        Err(_) => {
            tracing::info!("配置文件不存在，使用默认配置：{}", path.display());
            AppConfig::default()
        }
    }
}
