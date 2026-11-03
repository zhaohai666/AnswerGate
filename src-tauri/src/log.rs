//! 日志初始化：tracing + tracing-appender 按天滚动落地 `app_data_dir/logs/answer_gate.log`。
//!
//! 事件行结构（共享知识 #6）：`event=quiz_start|quiz_submit|grade|unlock|parent_unlock_fail`、
//! `level=...`、`score=...`、`result=pass|fail|launch_ok|launch_err`，家长可直接阅读。

use std::fs;
use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// 初始化日志。
///
/// - `enabled == false`（config [log].enable = false）：仅控制台输出，返回 None；
/// - 成功：返回 WorkerGuard（**调用方必须持有到进程结束**，否则日志缓冲不刷盘）。
pub fn init(app_data_dir: &Path, enabled: bool) -> Option<WorkerGuard> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    if !enabled {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .try_init();
        return None;
    }

    let log_dir: PathBuf = app_data_dir.join("logs");
    if let Err(e) = fs::create_dir_all(&log_dir) {
        eprintln!("[answergate] 创建日志目录失败 {}: {}", log_dir.display(), e);
        let _ = tracing_subscriber::fmt().with_env_filter(env_filter).try_init();
        return None;
    }

    let file_appender = tracing_appender::rolling::daily(&log_dir, "answer_gate.log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init();
    Some(guard)
}
