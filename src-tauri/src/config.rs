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
    // [monitor]：min_use_minutes 非法（≤0/非数值）回退 0.1
    if !(cfg.monitor.min_use_minutes.is_finite() && cfg.monitor.min_use_minutes > 0.0) {
        tracing::warn!("config.monitor.min_use_minutes 非法，回退默认值 0.1");
        cfg.monitor.min_use_minutes = 0.1;
    }
    // [monitor]：pause_key_code 为 0（无效虚拟键码）回退 49（空格）
    if cfg.monitor.pause_key_code == 0 {
        tracing::warn!("config.monitor.pause_key_code 为 0，回退默认值 49（空格）");
        cfg.monitor.pause_key_code = 49;
    }
    // [monitor]：watch_interval_sec 为 0 回退 3 秒（0 会导致忙轮询打满 CPU）
    if cfg.monitor.watch_interval_sec == 0 {
        tracing::warn!("config.monitor.watch_interval_sec 为 0，回退默认值 3 秒");
        cfg.monitor.watch_interval_sec = 3;
    }
    // [monitor]：timeup_question_count 为 0 回退 3 题（0 题 = 无人可答，等于永久卡死）
    if cfg.monitor.timeup_question_count == 0 {
        tracing::warn!("config.monitor.timeup_question_count 为 0，回退默认值 3");
        cfg.monitor.timeup_question_count = 3;
    }
    // [monitor]：timeup_pass_score 抬回 1.0（续期/拦截卷必须全对，需求明确，不可放宽）
    // 家长误把门槛改成 0.2 也无效——「3 题全对」是硬性要求。
    if cfg.monitor.timeup_pass_score < 1.0 {
        tracing::warn!(
            "config.monitor.timeup_pass_score 小于 1.0，抬回 1.0（续期必须全对）"
        );
        cfg.monitor.timeup_pass_score = 1.0;
    }
    // [[target_apps]]：use_minutes 低于下限/非法 → 回退下限并告警（架构 #4：最小 0.1）
    let min = cfg.monitor.min_use_minutes;
    for t in &mut cfg.target_apps {
        if !(t.use_minutes.is_finite() && t.use_minutes >= min) {
            tracing::warn!(
                target = %t.id,
                use_minutes = t.use_minutes,
                min = min,
                "target_apps.use_minutes 低于下限或非法，回退下限值"
            );
            t.use_minutes = min;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Level, QuizConfig};

    fn write_tmp(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load_or_default(&dir.path().join("no_such_config.toml"));
        assert_eq!(cfg.quiz, QuizConfig::default());
        assert!(cfg.target_apps.is_empty());
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let (_d, path) = write_tmp("[quiz]\nquestion_count = 10\n");
        let cfg = load_or_default(&path);
        assert_eq!(cfg.quiz.question_count, 10);
        assert_eq!(cfg.quiz.pass_score, 0.8);
        assert_eq!(cfg.quiz.max_retry, 2);
        assert_eq!(cfg.quiz.cool_down_sec, 120);
        assert_eq!(cfg.quiz.default_level, Level::Primary);
    }

    #[test]
    fn invalid_values_are_sanitized() {
        let (_d, path) =
            write_tmp("[quiz]\nquestion_count = 0\npass_score = 1.5\ncool_down_sec = 0\n");
        let cfg = load_or_default(&path);
        assert_eq!(cfg.quiz.question_count, 5);
        assert_eq!(cfg.quiz.pass_score, 0.8);
        assert_eq!(cfg.quiz.cool_down_sec, 120);
    }

    #[test]
    fn malformed_toml_falls_back_to_defaults() {
        let (_d, path) = write_tmp("[quiz\nbroken===");
        let cfg = load_or_default(&path);
        assert_eq!(cfg.quiz, QuizConfig::default());
    }

    #[test]
    fn targets_parsed_and_deduped() {
        let (_d, path) = write_tmp(
            r#"
            [[target_apps]]
            id = "a"
            name = "AppA"
            mac = "/Applications/A.app"
            [[target_apps]]
            id = "a"
            name = "AppA-dup"
            "#,
        );
        let cfg = load_or_default(&path);
        assert_eq!(cfg.target_apps.len(), 1);
        assert_eq!(cfg.target_apps[0].name, "AppA");
    }
}
