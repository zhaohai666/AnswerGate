//! 跨平台解锁模块：按 `cfg(target_os)` 三分支启动白名单目标软件（P0-05）。
//!
//! 平台差异只允许出现在本文件（桌面）与 plugins/answergate-launch（Android Kotlin）。
//! - Windows：`std::process::Command` 直接 spawn exe（先校验路径存在）
//! - macOS：`open -a <path>`（tauri.conf 已关闭沙箱，见 entitlements.plist）
//! - Android：经自研移动插件 answergate-launch 的 Kotlin Intent+setPackage 跳转

use crate::error::AppError;
use crate::model::TargetApp;
use tauri::AppHandle;

/// 启动目标软件统一入口。
///
/// Android 分支需要 AppHandle 调用移动插件；桌面分支忽略该参数。
/// 成功后由 command 层负责记日志并延迟退出应用。
pub fn launch_target(target: &TargetApp, app: &AppHandle) -> Result<(), AppError> {
    let spec = target.launch_spec().ok_or_else(|| {
        AppError::Unlock(format!(
            "目标「{}」未配置当前平台的启动路径/包名",
            target.name
        ))
    })?;
    platform_launch(spec, app)
}

#[cfg(target_os = "macos")]
fn platform_launch(spec: &str, _app: &AppHandle) -> Result<(), AppError> {
    launch_macos(spec)
}

#[cfg(target_os = "windows")]
fn platform_launch(spec: &str, _app: &AppHandle) -> Result<(), AppError> {
    launch_windows(spec)
}

#[cfg(target_os = "android")]
fn platform_launch(spec: &str, app: &AppHandle) -> Result<(), AppError> {
    answergate_launch::launch(app, spec.to_string())
        .map_err(|e| AppError::Unlock(format!("Android 跳转失败：{}", e)))
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
fn platform_launch(_spec: &str, _app: &AppHandle) -> Result<(), AppError> {
    Err(AppError::Unlock("该平台暂不支持解锁启动".to_string()))
}

/// macOS：`open -a <path>` 启动 .app。
///
/// 注意：不直接执行 app 内二进制；`open` 失败时返回非零退出码。
#[cfg(target_os = "macos")]
pub fn launch_macos(app_path: &str) -> Result<(), AppError> {
    if !std::path::Path::new(app_path).exists() {
        return Err(AppError::Unlock(format!("目标 App 不存在：{}", app_path)));
    }
    let status = std::process::Command::new("open")
        .arg("-a")
        .arg(app_path)
        .status()
        .map_err(|e| AppError::Unlock(format!("open -a 启动失败：{}", e)))?;
    if !status.success() {
        return Err(AppError::Unlock(format!(
            "open -a 返回非零状态（{}）：请检查路径与权限",
            status
        )));
    }
    tracing::info!(target = %app_path, event = "unlock", result = "launch_ok", "目标 App 已启动");
    Ok(())
}

/// Windows：spawn exe（路径转义由 Command 参数传递天然处理）。
///
/// 边界：仅应用层启动，无法阻止用户手动打开 exe（README 风险边界）。
#[cfg(target_os = "windows")]
pub fn launch_windows(exe_path: &str) -> Result<(), AppError> {
    if !std::path::Path::new(exe_path).exists() {
        return Err(AppError::Unlock(format!("目标 exe 不存在：{}", exe_path)));
    }
    std::process::Command::new(exe_path)
        .spawn()
        .map_err(|e| AppError::Unlock(format!("spawn exe 失败：{}", e)))?;
    tracing::info!(target = %exe_path, event = "unlock", result = "launch_ok", "目标 exe 已启动");
    Ok(())
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::*;

    #[test]
    fn missing_app_path_returns_unlock_error() {
        let err = launch_macos("/Applications/NoSuchApp_AnswerGate_XYZ.app").unwrap_err();
        assert_eq!(err.code(), "Unlock");
        assert!(err.to_string().contains("不存在"));
    }
}
