//! AnswerGate 应用装配层（lib 入口，桌面与移动共用）。
//!
//! 装配内容：日志初始化 → 题库/配置加载 → 门禁状态恢复 → AppState manage
//! → 全部 command 注册 → Android 启动插件注册 → 冷却计时器（按需）。

pub mod commands;
pub mod config;
pub mod error;
pub mod gate;
pub mod log;
pub mod model;
pub mod quiz;
pub mod unlock;

use tauri::Manager;

/// 解析题库目录：打包后位于 resource_dir；开发/测试回退到仓库相对路径。
fn resolve_bank_dir(handle: &tauri::AppHandle) -> std::path::PathBuf {
    use std::path::PathBuf;
    if let Ok(rd) = handle.path().resource_dir() {
        let nested = rd.join("resources").join("question_bank");
        if nested.is_dir() {
            return nested;
        }
        let flat = rd.join("question_bank");
        if flat.is_dir() {
            return flat;
        }
    }
    // tauri dev 的工作目录为 src-tauri/
    PathBuf::from("../resources/question_bank")
}

/// 应用统一入口。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(answergate_launch::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_config,
            commands::list_targets,
            commands::start_quiz,
            commands::retry_quiz,
            commands::submit_answers,
            commands::launch_target,
            commands::get_gate_state,
            commands::parent_unlock,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            // 1) 日志（guard 需活到进程结束：进程生命周期对象，forget 安全）
            let app_data = handle.path().app_data_dir().map_err(|e| {
                Box::new(std::io::Error::other(format!(
                    "无法获取 app_data_dir：{e}"
                ))) as Box<dyn std::error::Error>
            })?;
            if let Some(guard) = log::init(&app_data, true) {
                std::mem::forget(guard);
            }
            // 2) 题库/配置/门禁状态
            let bank_dir = resolve_bank_dir(&handle);
            let state = commands::AppState::new(app_data, &bank_dir)?;
            // 3) 重启恢复：冷却中则立即启动倒计时推送（防重启绕过，PRD P0-06）
            if state.gate.lock().unwrap().phase == gate::GatePhase::Cooldown {
                commands::spawn_cooldown_ticker(handle);
            }
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("AnswerGate 启动失败：无法初始化 Tauri 运行时");
}
