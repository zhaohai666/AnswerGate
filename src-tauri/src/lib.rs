//! AnswerGate 应用装配层（lib 入口，桌面与移动共用）。
//!
//! 装配内容：日志初始化 → 题库/配置加载 → 门禁状态恢复 → AppState manage
//! → 全部 command 注册 → Android 启动插件注册 → 冷却/使用中计时器（按需）
//! → 单实例锁 + 关窗拦截（v2 常驻化，仅桌面）。

pub mod clock;
pub mod commands;
pub mod config;
pub mod error;
pub mod gate;
pub mod log;
pub mod model;
pub mod monitor;
pub mod quiz;
pub mod unlock;

use tauri::{Emitter, Manager};

/// 解析题库目录：打包后位于 resource_dir（tauri bundler 会把 "../x" 声明映射为
/// bundle 内 "_up_/x"，故须探测 _up_ 前缀布局）；开发/测试回退到仓库相对路径。
fn resolve_bank_dir(handle: &tauri::AppHandle) -> std::path::PathBuf {
    use std::path::PathBuf;
    if let Ok(rd) = handle.path().resource_dir() {
        // tauri.conf.json bundle.resources 声明为 "../resources/..."，
        // 打包产物实际位于 Contents/Resources/_up_/resources/question_bank/
        let bundled_nested = rd.join("_up_").join("resources").join("question_bank");
        if bundled_nested.is_dir() {
            return bundled_nested;
        }
        let bundled_flat = rd.join("_up_").join("question_bank");
        if bundled_flat.is_dir() {
            return bundled_flat;
        }
        // 兜底：resources 声明不带 "../" 时的两种布局
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
    let builder = tauri::Builder::default()
        .plugin(answergate_launch::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_config,
            commands::list_targets,
            commands::start_quiz,
            commands::retry_quiz,
            commands::submit_answers,
            commands::launch_target,
            commands::extend_and_relaunch,
            commands::get_monitor_state,
            commands::stop_monitor,
            commands::confirm_exit,
            commands::get_active_paper,
            commands::get_gate_state,
            commands::parent_unlock,
            commands::frontend_log,
        ]);

    // 桌面端：单实例锁（C12，二次启动聚焦已有窗口）
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_focus();
        }
    }));

    builder
        .setup(|app| {
            let handle = app.handle().clone();
            // 1) 日志（同步写：每条立即落盘，进程被杀也不丢日志）
            let app_data = handle.path().app_data_dir().map_err(|e| {
                Box::new(std::io::Error::other(format!(
                    "无法获取 app_data_dir：{e}"
                ))) as Box<dyn std::error::Error>
            })?;
            log::init(&app_data, true);
            // 2) 题库/配置/门禁状态
            let bank_dir = resolve_bank_dir(&handle);
            let state = commands::AppState::new(app_data, &bank_dir)?;
            // 3) 判定需启动的计时器（必须在 manage 之前取相位/字段，
            //    计时器线程内部会 app.state::<AppState>()， manage 后再 spawn）
            let cooldown_resume =
                state.gate.lock().unwrap().phase == gate::GatePhase::Cooldown;
            let using_resume = commands::revalidate_using_on_startup(&state, commands::now());
            let using_target = if using_resume {
                state.gate.lock().unwrap().managed_target_id.clone()
            } else {
                None
            };
            app.manage(state);
            // 4) 重启恢复：冷却中则立即启动倒计时推送（防重启绕过，PRD P0-06）
            if cooldown_resume {
                commands::spawn_cooldown_ticker(handle.clone());
            }
            // 5) v2 重启恢复：使用中则重验句柄/剩余时长，成功则恢复监控（C10）
            if using_resume {
                let managed = handle.state::<commands::AppState>();
                let round_sec = commands::round_sec_for(&managed, using_target.as_deref());
                commands::spawn_using_monitor(handle.clone(), round_sec);
            }
            // 6) v3 独立进程监控：探测「绕过本程序启动」的目标软件并强制答题
            commands::spawn_watchdog(handle.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // v2 常驻化：拦截关窗 → 前端二次确认弹窗（确认后调 confirm_exit）
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("gate://close-requested", serde_json::json!({}));
            }
        })
        .run(tauri::generate_context!())
        .expect("AnswerGate 启动失败：无法初始化 Tauri 运行时");
}
