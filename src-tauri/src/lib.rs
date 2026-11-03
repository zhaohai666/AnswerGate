//! AnswerGate 应用装配层（lib 入口，桌面与移动共用）。
//!
//! T02：核心内核模块挂载（纯逻辑，平台无关）。
//! T04 将在 run() 中注册全部业务 commands、管理 AppState、初始化日志与事件推送。

pub mod config;
pub mod error;
pub mod gate;
pub mod log;
pub mod model;
pub mod quiz;
pub mod unlock;

/// 应用统一入口。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(answergate_launch::init())
        .setup(|_app| {
            // T04：初始化日志（log.rs）、加载配置与题库（config.rs / quiz.rs）、
            // 恢复门禁状态（gate.rs）、管理 AppState、注册 commands。
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("AnswerGate 启动失败：无法初始化 Tauri 运行时");
}
