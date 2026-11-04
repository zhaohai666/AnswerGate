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
#[cfg(target_os = "macos")]
use crate::monitor::alive;
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
        tracing::error!(app_path, event = "unlock_error", "目标 App 路径不存在");
        return Err(AppError::Unlock(format!("目标 App 不存在：{}", app_path)));
    }
    let status = std::process::Command::new("open")
        .arg("-a")
        .arg(app_path)
        .status()
        .map_err(|e| {
            tracing::error!(app_path, error = %e, event = "unlock_error", "open 进程启动失败");
            AppError::Unlock(format!("open -a 启动失败：{}", e))
        })?;
    if !status.success() {
        tracing::error!(
            app_path,
            code = status.code(),
            event = "unlock_error",
            "open -a 返回非零状态"
        );
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

/// 探测目标软件当前是否在运行（v3 独立进程监控用）。
///
/// 与 `pid_of_app` 的区别：**单次快速探测**，不做 12s 重试窗口、不做 500ms 稳定性复核——
/// watchdog 每隔数秒轮询一次，必须是低开销的「在不在」判断，而非「刚启动时的句柄获取」。
///
/// - macOS：精确 pgrep 主二进制路径 + 主进程校验（排除 Helper）；
/// - Windows：tasklist 按 exe 名过滤；
/// - Android：无法探测其他应用，恒返回 `None`（系统限制，见 PRD 边界）。
pub fn probe_running_pid(target: &TargetApp) -> Option<i32> {
    let spec = target.launch_spec()?;
    platform_probe_running_pid(spec)
}

#[cfg(target_os = "macos")]
fn platform_probe_running_pid(spec: &str) -> Option<i32> {
    let exec_name = bundle_executable_name(spec)?;
    let main_bin = format!("{}/Contents/MacOS/{}", spec, exec_name);
    // 精确 pgrep：命中即为主进程路径，天然排除 Helper 子进程
    let pid = pgrep_exact(&main_bin)?;
    match is_main_app_process(pid, spec) {
        Some(true) => Some(pid),
        Some(false) => {
            // 确认不是主进程：主进程刚退出、Helper 尚存 → 宽匹配兜底
            pgrep_broad_then_validate(spec)
        }
        None => {
            // 探测不可用（`ps` 被系统策略禁用）——精确路径本身已唯一标识主进程
            // （Helper 在 Contents/Frameworks/ 下，不含 Contents/MacOS/），故信任命中。
            tracing::debug!(
                pid,
                target = %spec,
                event = "watch",
                result = "pid_probe_unverified",
                "主进程校验不可用（ps 受限），采信精确路径命中"
            );
            Some(pid)
        }
    }
}

#[cfg(target_os = "windows")]
fn platform_probe_running_pid(spec: &str) -> Option<i32> {
    // Windows 按 exe 文件名过滤 tasklist
    let exe_name = std::path::Path::new(spec)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())?;
    let filter = format!("IMAGENAME eq {}", exe_name);
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &filter, "/NH", "/FO", "CSV"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // CSV 行格式："name.exe","pid","session","session#","mem"
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split(',');
            let name = parts.next()?.trim().trim_matches('"');
            if !name.eq_ignore_ascii_case(&exe_name) {
                return None;
            }
            parts.next()?.trim().trim_matches('"').parse::<i32>().ok()
        })
        .next()
}

#[cfg(target_os = "android")]
fn platform_probe_running_pid(_spec: &str) -> Option<i32> {
    // Android 沙箱限制：普通应用无法枚举其他应用的进程（系统限制，见 PRD Q3）。
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
fn platform_probe_running_pid(_spec: &str) -> Option<i32> {
    None
}

/// macOS：按 app 路径探测运行中主进程的 PID（v2 加固）。
///
/// 多策略冗余（任一命中即返回），每条策略命中后都做"存活 + 500ms 稳定性复核"
/// 防取到瞬时/正在退出的进程：
/// ① **精确 pgrep**：`pgrep -f "<app>/Contents/MacOS/<CFBundleExecutable>"`
///    ——匹配主二进制完整路径，天然排除 Helper（Helper 在 Contents/Frameworks 下），
///    无需辅助功能权限，为主路径；
/// ② osascript System Events：`unix id of (first process whose name is "<名>")`
///    ——原 `unix id of application` 形式在本机 AppleScript 环境中为**语法错误 (-2740)**，
///    已改为 System Events tell 形式（需辅助功能权限，AnswerGate 已授权）；
/// ③ 兜底宽 pgrep + 主进程校验（`ps` 校验 cmdline 含 `Contents/MacOS/` 且非 Helper）。
///
/// `open -a` 由 launchd 托管，主进程完全启动前可能尚未注册；以 200ms 间隔重试
/// 最多 [`PROBE_ATTEMPTS`] 次（约 12s 窗口）：重型应用（如 B 站 Electron）冷启动慢。
#[cfg(target_os = "macos")]
pub fn pid_of_app(app_path: &str) -> Option<i32> {
    let app_name = std::path::Path::new(app_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| app_path.to_string());
    let exec_name = bundle_executable_name(app_path).unwrap_or_else(|| app_name.clone());
    let main_bin = format!("{}/Contents/MacOS/{}", app_path, exec_name);
    tracing::info!(
        target = %app_path,
        exec = %exec_name,
        event = "monitor",
        result = "pid_probe_start",
        "开始探测目标主进程 PID"
    );
    for attempt in 0..PROBE_ATTEMPTS {
        // ① 精确 pgrep 主二进制路径（首选，无需辅助功能权限）
        if let Some(pid) = pgrep_exact(&main_bin) {
            if stabilize(pid) {
                tracing::info!(pid, target = %app_path, event = "monitor", result = "pid_acquired", "精确 pgrep 取得主进程句柄");
                return Some(pid);
            }
        }
        // ② osascript System Events（需辅助功能权限）
        if let Some(pid) = pid_via_osascript_sys(&app_name) {
            if stabilize(pid) {
                tracing::info!(pid, target = %app_path, event = "monitor", result = "pid_acquired", "osascript 取得主进程句柄");
                return Some(pid);
            }
        }
        // ③ 兜底宽 pgrep + 主进程校验
        if let Some(pid) = pgrep_broad_then_validate(app_path) {
            if stabilize(pid) {
                tracing::info!(pid, target = %app_path, event = "monitor", result = "pid_acquired", "宽 pgrep+校验取得主进程句柄");
                return Some(pid);
            }
        }
        if attempt == 0 {
            tracing::debug!(target = %app_path, event = "monitor", "首次探测未命中，持续重试中（冷启动/尚未注册）");
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    tracing::warn!(
        target = %app_path,
        event = "monitor",
        result = "pid_timeout",
        "12s 窗口内未取得 PID，放弃纳管（宁不管控不误杀）"
    );
    None
}

/// osascript System Events 形式（修复原 `unix id of application` 的 -2740 语法错误）。
/// 需辅助功能权限；缺权限返回 -10004（非成功），调用方忽略即可。
#[cfg(target_os = "macos")]
fn pid_via_osascript_sys(app_name: &str) -> Option<i32> {
    let script = format!(
        "tell application \"System Events\" to get unix id of (first process whose name is \"{}\")",
        app_name
    );
    let out = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse::<i32>().ok()
}

/// 兜底：宽 pgrep 匹配 app 路径（正则转义），候选逐一做主进程校验（排除 Helper）。
#[cfg(target_os = "macos")]
fn pgrep_broad_then_validate(app_path: &str) -> Option<i32> {
    let pattern = regex_escape(app_path);
    let out = std::process::Command::new("pgrep")
        .args(["-f", &pattern])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .find(|pid| is_main_app_process(*pid, app_path).unwrap_or(false))
}

/// 精确 pgrep：匹配主二进制完整路径（正则转义后），天然排除 Helper 子进程。
#[cfg(target_os = "macos")]
fn pgrep_exact(main_bin: &str) -> Option<i32> {
    let pattern = regex_escape(main_bin);
    let out = std::process::Command::new("pgrep")
        .args(["-f", &pattern])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .next()
}

/// 正则转义（pgrep -f 按正则匹配命令行，路径中的 `.` 等需转义以防歧义匹配）。
#[cfg(target_os = "macos")]
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if ".+*?()[]{}|^$\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 稳定性复核：当前存活 + 500ms 后依旧存活，避免取到瞬时/正在退出的进程。
#[cfg(target_os = "macos")]
fn stabilize(pid: i32) -> bool {
    if !alive(pid) {
        return false;
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    alive(pid)
}

/// 读取 .app 的 CFBundleExecutable（Info.plist），用于构造精确主二进制路径。
#[cfg(target_os = "macos")]
fn bundle_executable_name(app_path: &str) -> Option<String> {
    let plist = format!("{}/Contents/Info.plist", app_path);
    let out = std::process::Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleExecutable", &plist])
        .output()
        .ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

/// 探测窗口重试次数（200ms × 60 ≈ 12s，覆盖重型应用冷启动）。
#[cfg(target_os = "macos")]
const PROBE_ATTEMPTS: usize = 60;

/// 校验 pid 是否为 app 的主进程：命令行必须是
/// `<app_path>/Contents/MacOS/<可执行名>` 且不含 "Helper"。
///
/// ⚠️ 返回三态（`Option<bool>`）：
/// - `Some(true)`  = 确认是主进程；
/// - `Some(false)` = 确认不是主进程（Helper 等）；
/// - `None`       = **无法判定**（`ps` 被系统策略禁用，真机实测遇到过）。
///
/// 无法判定时调用方应保守处理：精确 pgrep 命中 `<app>/Contents/MacOS/<exe>`
/// 这一完整路径本身已唯一标识主进程（Helper 位于 `Contents/Frameworks/` 下），
/// 故可采信该命中。
#[cfg(target_os = "macos")]
fn is_main_app_process(pid: i32, app_path: &str) -> Option<bool> {
    // ① 标准路径：ps 取命令行
    if let Some(cmdline) = process_cmdline_via_ps(pid) {
        return Some(check_main_cmdline(&cmdline, app_path));
    }
    // ② 降级路径：ps 被禁用 → 用 `pgrep -fl` 拿该 app 下所有进程的完整 argv
    let listed = pgrep_list_with_args(&regex_escape(app_path))?;
    listed
        .iter()
        .find(|(p, _)| *p == pid)
        .map(|(_, cmd)| check_main_cmdline(cmd, app_path))
}

/// 命令行是否满足「主进程」特征。
#[cfg(target_os = "macos")]
fn check_main_cmdline(cmdline: &str, app_path: &str) -> bool {
    let is_main = cmdline.starts_with(app_path)
        && cmdline.contains("Contents/MacOS/")
        && !cmdline.contains("Helper")
        && !cmdline.contains("helper");
    if !is_main {
        tracing::debug!(cmdline = %cmdline, "pgrep 候选非主进程，跳过");
    }
    is_main
}

/// 用 `ps -p <pid> -o command=` 取进程完整命令行；`ps` 不可用时返回 None。
#[cfg(target_os = "macos")]
fn process_cmdline_via_ps(pid: i32) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 用 `pgrep -fl <pattern>` 列出匹配进程的 `(pid, 完整 argv)`。
///
/// `ps`/`top` 被系统策略禁用时，`pgrep` 仍是唯一可用的进程枚举工具
/// （macOS 自带，不走 shell 权限检查路径）。
#[cfg(target_os = "macos")]
fn pgrep_list_with_args(pattern: &str) -> Option<Vec<(i32, String)>> {
    let out = std::process::Command::new("pgrep")
        .args(["-fl", pattern])
        .output()
        .ok()?;
    if !out.status.success() {
        // pgrep 无匹配时退出码为 1 —— 这不是错误，只是没有命中
        return Some(Vec::new());
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let (pid, rest) = line.split_once(' ')?;
                Some((pid.trim().parse::<i32>().ok()?, rest.trim().to_string()))
            })
            .collect(),
    )
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

    #[test]
    fn regex_escape_escapes_special_chars() {
        assert_eq!(regex_escape("/Applications/App.app/Contents/MacOS/x"),
                   "/Applications/App\\.app/Contents/MacOS/x");
        assert_eq!(regex_escape("a.b+c*d"), "a\\.b\\+c\\*d");
    }

    #[test]
    fn pgrep_exact_finds_running_process() {
        // 启动一个长时 sleep，验证精确 pgrep 能命中其主二进制路径
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep 失败");
        let _ = child.id();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let found = pgrep_exact("/bin/sleep");
        let _ = child.kill();
        assert!(
            found.is_some(),
            "精确 pgrep 应找到 /bin/sleep 主进程（验证 PID 探测主路径可用）"
        );
    }
}
