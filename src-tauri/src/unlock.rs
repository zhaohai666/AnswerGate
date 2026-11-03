//! 跨平台解锁模块：按 `cfg(target_os)` 三分支启动白名单目标软件（P0-05）。
//!
//! 平台差异只允许出现在本文件（桌面）与 plugins/answergate-launch（Android Kotlin）。
//! - Windows：`std::process::Command` 直接 spawn exe（先校验路径存在）
//! - macOS：`open -a <path>`（tauri.conf 已关闭沙箱，见 entitlements.plist）
//! - Android：经自研移动插件 answergate-launch 的 Kotlin Intent+setPackage 跳转
//!
//! v2 增量（T01）：新增 `launch_target_handle` / `launch_macos_managed` /
//! `launch_windows_managed`——启动成功后尝试取得目标进程 PID 供 monitor 纳管；
//! 既有 `launch_target` 行为与签名不变（返回值兼容：新增方法而非破坏式修改）。

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

/// 启动目标软件并尝试取得进程 PID（v2 T01 纳管入口）。
///
/// - Windows：spawn 后直接返回 `Child::id()`（天然可靠）
/// - macOS：`open -a` 后经 osascript/pgrep 探测；5s 窗口取不到返回 `None`
///   （U7：宁可不管控不误杀）
/// - Android：不取句柄（U11 降级），恒返回 `None`
pub fn launch_target_handle(
    target: &TargetApp,
    app: &AppHandle,
) -> Result<Option<i32>, AppError> {
    let spec = target.launch_spec().ok_or_else(|| {
        AppError::Unlock(format!(
            "目标「{}」未配置当前平台的启动路径/包名",
            target.name
        ))
    })?;
    platform_launch_handle(spec, app)
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

#[cfg(target_os = "macos")]
fn platform_launch_handle(spec: &str, _app: &AppHandle) -> Result<Option<i32>, AppError> {
    launch_macos_managed(spec)
}

#[cfg(target_os = "windows")]
fn platform_launch_handle(spec: &str, _app: &AppHandle) -> Result<Option<i32>, AppError> {
    launch_windows_managed(spec)
}

// Android：U11 降级——仅提示模式，不纳管句柄
#[cfg(target_os = "android")]
fn platform_launch_handle(spec: &str, app: &AppHandle) -> Result<Option<i32>, AppError> {
    platform_launch(spec, app)?;
    Ok(None)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
fn platform_launch_handle(_spec: &str, _app: &AppHandle) -> Result<Option<i32>, AppError> {
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

/// macOS：`open -a` 启动后探测 PID（v2 T01）。
///
/// 探测失败不影响启动结果：返回 `Ok(None)` 表示"已启动但未纳管"（U7 宁不管控不误杀）。
#[cfg(target_os = "macos")]
pub fn launch_macos_managed(app_path: &str) -> Result<Option<i32>, AppError> {
    launch_macos(app_path)?;
    Ok(pid_of_app(app_path))
}

/// macOS：按 app 路径探测运行中进程的 PID。
///
/// 启动竞态处理（U7/U9）：launchd 托管进程在 App 完全启动前 `unix id` 无值，
/// 以 200ms 间隔重试最多 25 次（5s 窗口）：
/// ① 主选 osascript `unix id of application "<名字>"`（按名字精确匹配）；
/// ② 兜底 `pgrep -f <完整 app 路径>`（匹配完整路径，避免同名误杀）。
/// 均取不到返回 `None`。
#[cfg(target_os = "macos")]
pub fn pid_of_app(app_path: &str) -> Option<i32> {
    let app_name = std::path::Path::new(app_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| app_path.to_string());
    for _ in 0..25 {
        if let Some(pid) = pid_via_osascript(&app_name).or_else(|| pid_via_pgrep(app_path)) {
            tracing::info!(
                pid,
                target = %app_path,
                event = "monitor",
                result = "pid_acquired",
                "已取得目标进程句柄"
            );
            return Some(pid);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    tracing::warn!(
        target = %app_path,
        event = "monitor",
        result = "pid_timeout",
        "5s 窗口内未取得 PID，放弃纳管（宁不管控不误杀）"
    );
    None
}

/// osascript 主路径：`unix id of application "<name>"`。
#[cfg(target_os = "macos")]
fn pid_via_osascript(app_name: &str) -> Option<i32> {
    let script = format!("unix id of application \"{}\"", app_name);
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse::<i32>().ok()
}

/// pgrep 兜底：`pgrep -f <完整 app 路径>`（多匹配取首个）。
#[cfg(target_os = "macos")]
fn pid_via_pgrep(app_path: &str) -> Option<i32> {
    let out = std::process::Command::new("pgrep")
        .arg("-f")
        .arg(app_path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .next()
        .and_then(|line| line.trim().parse::<i32>().ok())
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

/// Windows：spawn exe 并返回子进程 PID（v2 T01：`Child::id()` 天然可靠）。
#[cfg(target_os = "windows")]
pub fn launch_windows_managed(exe_path: &str) -> Result<Option<i32>, AppError> {
    if !std::path::Path::new(exe_path).exists() {
        return Err(AppError::Unlock(format!("目标 exe 不存在：{}", exe_path)));
    }
    let child = std::process::Command::new(exe_path)
        .spawn()
        .map_err(|e| AppError::Unlock(format!("spawn exe 失败：{}", e)))?;
    let pid = child.id() as i32;
    tracing::info!(
        pid,
        target = %exe_path,
        event = "unlock",
        result = "launch_ok",
        "目标 exe 已启动并纳管"
    );
    Ok(Some(pid))
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
