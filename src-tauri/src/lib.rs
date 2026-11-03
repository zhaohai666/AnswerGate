//! AnswerGate 应用装配层（lib 入口，桌面与移动共用）。
//!
//! T01 阶段为最小可运行骨架：仅注册 Builder 并启动窗口。
//! T04 将在此注册全部业务 commands、管理 AppState、初始化日志与事件推送。

/// 应用统一入口。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            // T04：初始化日志（log.rs）、加载配置与题库（config.rs / quiz.rs）、
            // 恢复门禁状态（gate.rs）、管理 AppState。
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("AnswerGate 启动失败：无法初始化 Tauri 运行时");
}
