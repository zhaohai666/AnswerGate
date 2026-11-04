//! 日志初始化：tracing + tracing-appender 按天滚动落地 `app_data_dir/logs/answer_gate.log`。
//!
//! 事件行结构（共享知识 #6）：`event=quiz_start|quiz_submit|grade|unlock|parent_unlock_fail`、
//! `level=...`、`score=...`、`result=pass|fail|launch_ok|launch_err`，家长可直接阅读。
//!
//! ⚠️ v3 变更：**去掉 `non_blocking` 缓冲，改为直接阻塞写**。
//! 原实现用 `tracing_appender::non_blocking`（默认缓冲 128 行才落盘），
//! 导致进程被 kill / 异常退出时缓冲区里的日志全部丢失——
//! 而「进程被 kill」「pid 探测失败」恰恰是家长最需要排查的场景，
//! 却是最容易丢日志的场景。改为同步写后每条日志立即落盘。
//! 日志量很小（watchdog 每 3s 一条），同步写的开销可忽略。

use std::fs;
use std::path::{Path, PathBuf};

use tracing_subscriber::EnvFilter;

/// 初始化日志。
///
/// - `enabled == false`（config [log].enable = false）：仅控制台输出；
/// - 成功：文件日志立即生效（每条同步落盘，无需持有 guard）。
pub fn init(app_data_dir: &Path, enabled: bool) -> bool {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    if !enabled {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .try_init();
        return false;
    }

    let log_dir: PathBuf = app_data_dir.join("logs");
    if let Err(e) = fs::create_dir_all(&log_dir) {
        eprintln!("[answergate] 创建日志目录失败 {}: {}", log_dir.display(), e);
        let _ = tracing_subscriber::fmt().with_env_filter(env_filter).try_init();
        return false;
    }

    let file_appender = tracing_appender::rolling::daily(&log_dir, "answer_gate.log");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(file_appender)
        .with_ansi(false)
        .try_init();
    true
}
