//! 进程纳管与计时核心（T01 竖切：「取 PID → 计时 → kill」最短链路）。
//!
//! - [`ManagedProcess`]：句柄记录（pid / 目标信息 / 轮次开始时间）
//! - [`alive`]：跨平台存活检查（unix `kill -0`，Windows `tasklist` 过滤）
//! - [`terminate`]：两段式终止（unix SIGTERM → 2s → SIGKILL；Windows `taskkill /T /F`）
//! - [`remaining_sec`]：计时到期检测纯函数（供 monitor 循环与单测复用）
//!
//! 平台差异仍只允许出现在本文件与 unlock.rs（架构约定）；不引入 sysinfo（架构 1.2 决策）。

use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::model::PauseState;

/// 受纳管的目标进程句柄（仅由本程序拉起的进程才被记录）。
#[derive(Debug, Clone)]
pub struct ManagedProcess {
    /// 目标进程 PID（macOS 经 osascript/pgrep 取得，Windows 为 `Child::id()`）。
    pub pid: i32,
    /// 目标展示名（config target_apps.name）。
    pub target_name: String,
    /// 当前平台启动规格（mac 路径 / win exe / android 包名），日志诊断用。
    pub target_spec: String,
    /// 本轮开始时间（Unix epoch 秒），持久化依据（防重启绕过，C10）。
    pub round_started_epoch: u64,
}

impl ManagedProcess {
    /// 构造纳管记录；`now_epoch` 由调用方经 Clock 取得。
    pub fn new(
        pid: i32,
        target_name: impl Into<String>,
        target_spec: impl Into<String>,
        now_epoch: u64,
    ) -> Self {
        ManagedProcess {
            pid,
            target_name: target_name.into(),
            target_spec: target_spec.into(),
            round_started_epoch: now_epoch,
        }
    }

    /// 目标进程是否仍存活。
    pub fn is_alive(&self) -> bool {
        alive(self.pid)
    }

    /// 终止目标进程（两段式）；返回是否确认死亡。
    pub fn terminate(&self) -> bool {
        terminate(self.pid)
    }
}

/// 进程存活检查（pid ≤ 0 一律视为不存活）。
pub fn alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        alive_windows(pid)
    }
    #[cfg(not(target_os = "windows"))]
    {
        alive_unix(pid)
    }
}

#[cfg(not(target_os = "windows"))]
fn alive_unix(pid: i32) -> bool {
    // kill -0：只探测不发信号；进程存在（含权限可及）返回成功。
    // 僵尸边界：kill -0 无法区分僵尸态，但纳管目标经 `open -a` 由 launchd 托管，
    // 死后由 launchd 立即收尸，不会长期滞留僵尸态；本程序自行 spawn 的子进程
    // （Windows 路径）由 Child::try_wait 收尸（T03 接入），故此语义成立。
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn alive_windows(pid: i32) -> bool {
    // tasklist 按 PID 过滤；T01 占位实现（保证编译通过，真机回归在 T05）
    let filter = format!("PID eq {}", pid);
    Command::new("tasklist")
        .args(["/FI", &filter, "/NH", "/FO", "CSV"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

/// 终止目标进程。
///
/// - unix：SIGTERM → 最多 2s 轮询（100ms 间隔）→ 仍存活则 SIGKILL（架构 U8 两段式）
/// - Windows：`taskkill /PID <pid> /T /F`（进程树终止，C9）
///
/// 返回是否确认进程已死亡；失败仅记 `kill_failed` 日志（U8：窗口置顶仍保管控）。
pub fn terminate(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        terminate_windows(pid)
    }
    #[cfg(not(target_os = "windows"))]
    {
        terminate_unix(pid)
    }
}

#[cfg(not(target_os = "windows"))]
fn terminate_unix(pid: i32) -> bool {
    // 第一段：SIGTERM（kill 默认信号），给进程优雅退出机会
    let _ = Command::new("kill").arg(pid.to_string()).status();
    // 最多等 2s，每 100ms 探测一次
    for _ in 0..20 {
        if !alive(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    // 第二段：SIGKILL 强杀
    let _ = Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .status();
    let dead = !alive(pid);
    if !dead {
        tracing::warn!(pid, event = "kill_failed", "SIGKILL 后进程仍存活");
    }
    dead
}

#[cfg(target_os = "windows")]
fn terminate_windows(pid: i32) -> bool {
    // /T 终止进程树（主+子进程），/F 强制（C9）
    let ok = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        tracing::warn!(pid, event = "kill_failed", "taskkill /T /F 失败");
    }
    ok
}

// ---------- v2.1 pause 模式：模拟按键暂停 / 恢复（软管控，不退出程序） ----------

/// 构造「把指定 PID 的进程置前」的 AppleScript 表达式（纯函数，供测试与实现复用）。
#[cfg(not(target_os = "windows"))]
pub fn osascript_focus_args(pid: i32) -> String {
    format!(
        "tell application \"System Events\" to set frontmost of (first process whose unix id is {}) to true",
        pid
    )
}

/// 构造「发送一次虚拟键码按键」的 AppleScript 表达式（纯函数，供测试与实现复用）。
#[cfg(not(target_os = "windows"))]
pub fn osascript_keycode_args(key_code: u16) -> String {
    format!("tell application \"System Events\" to key code {}", key_code)
}

/// 对目标进程模拟一次按键（pause/resume 共用：空格类键码即切换暂停/播放）。
///
/// 流程（macOS）：先把目标进程置前（按 pid 定位，避免依赖进程名）→ 短暂等待
/// 让其成为前台 → 发送虚拟键码。**需要系统授予「辅助功能」权限**，未授权时
/// osascript 失败，调用方应回退 terminate（见 commands::handle_timeout）。
///
/// Windows：全局发送媒体播放/暂停键（VK_MEDIA_PLAY_PAUSE = 0xB3，SendInput），
/// 无需焦点，`key_code` 参数忽略。
///
/// 返回按键是否发送成功（不校验目标软件是否真的暂停——应用层边界）。
pub fn toggle_playback_key(pid: i32, key_code: u16) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        toggle_playback_key_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        toggle_playback_key_unix(pid, key_code)
    }
}

#[cfg(not(target_os = "windows"))]
fn toggle_playback_key_unix(pid: i32, key_code: u16) -> bool {
    let focus = osascript_focus_args(pid);
    let key = osascript_keycode_args(key_code);
    let ok = Command::new("osascript")
        .args(["-e", &focus, "-e", "delay 0.3", "-e", &key])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        tracing::warn!(
            pid,
            key_code,
            event = "playback_key_failed",
            "模拟按键失败（大概率缺少「辅助功能」权限），请系统设置 → 隐私与安全性 → 辅助功能 中勾选 AnswerGate"
        );
    }
    ok
}

#[cfg(target_os = "windows")]
fn toggle_playback_key_windows() -> bool {
    // VK_MEDIA_PLAY_PAUSE（0xB3）media key 经 SendInput 发送，无需目标前台焦点。
    // T01 同款占位策略：保证编译通过，真机回归在 T05（Windows 真机）。
    let script = "\
$sig='[DllImport(\"user32.dll\")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtra);'; \
Add-Type -MemberDefinition $sig -Name K -Namespace W; \
[W.K]::keybd_event(0xB3,0,0,[UIntPtr]::Zero); \
[W.K]::keybd_event(0xB3,0,2,[UIntPtr]::Zero);";
    let ok = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        tracing::warn!(event = "playback_key_failed", "Windows 媒体键发送失败");
    }
    ok
}

/// pause 模式超时动作：把目标软件置前并模拟一次暂停键。
/// 进程保持存活（不退出）；返回按键是否发送成功，失败由调用方决定回退策略。
///
/// **注意（v3）**：空格是「切换」语义，本函数只应在「确认视频正在播放」时调用
/// （由调用方保证状态机一致性）。防绕过重暂停请用 [`repause_if_playing`]。
pub fn pause_target(pid: i32, key_code: u16) -> bool {
    let ok = toggle_playback_key(pid, key_code);
    if ok {
        tracing::info!(pid, key_code, event = "target_paused", "已发送暂停按键，目标进程保持存活");
    }
    ok
}

/// pause 模式续期动作：再按一次暂停/播放键恢复播放（与暂停是同一个切换键）。
///
/// 答题通过后调用：把目标置前（"弹出"）+ 发送切换键（"开始播放"）。
pub fn resume_target(pid: i32, key_code: u16) -> bool {
    let ok = toggle_playback_key(pid, key_code);
    if ok {
        tracing::info!(pid, key_code, event = "target_resumed", "已发送恢复按键，继续本轮计时");
    }
    ok
}

/// 把目标窗口置前（"弹出"到前台），**不发送任何按键**。
///
/// 用于「视频本来就在播放、只是需要把孩子带回 B 站窗口」的场景：
/// 此时若发切换键反而会把视频暂停。
pub fn bring_to_front(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        // AppActivate(进程 pid) 可激活窗口；失败不致命（仅日志）。
        let script = "\
Add-Type -AssemblyName Microsoft.VisualBasic; \
[Microsoft.VisualBasic.Interaction]::AppActivate($args[0]);";
        let ok = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script, &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            tracing::warn!(pid, event = "bring_to_front_failed", "Windows 激活目标窗口失败");
        }
        ok
    }
    #[cfg(not(target_os = "windows"))]
    {
        let focus = osascript_focus_args(pid);
        let ok = Command::new("osascript")
            .args(["-e", &focus])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            tracing::warn!(pid, event = "bring_to_front_failed", "目标窗口置前失败");
        }
        ok
    }
}

/// 答题通过后的「弹出 + 开始播放」（v3）。
///
/// 空格是**切换**语义，故必须先确定视频当前是不是暂停态，才能决定发不发切换键：
///
/// - `PauseState::Paused`：确认视频已暂停 → 发一次切换键恢复播放（并把窗口置前）；
/// - `PauseState::Playing`：视频仍在播 → **只置前不切换**，否则会把视频反向暂停；
/// - `PauseState::Unknown`：探测不可用 → 发一次切换键（与暂停时的动作对称）。
///
/// 返回 true = 本次确实发送了切换键（视频被恢复播放）。
pub fn resume_if_paused(pid: i32, key_code: u16, pause_state: PauseState) -> bool {
    match pause_state {
        PauseState::Paused | PauseState::Unknown => {
            let ok = toggle_playback_key(pid, key_code);
            tracing::info!(pid, ok, event = "target_resumed", "发送切换键恢复播放并置前窗口");
            ok
        }
        PauseState::Playing => {
            // 仍在播放：只置前，绝不发切换键
            let ok = bring_to_front(pid);
            tracing::info!(pid, ok, event = "target_resumed_no_toggle", "目标仍在播放，仅置前窗口（不发送切换键）");
            false
        }
    }
}

/// 「确保目标处于暂停态」（v3）：先判定播放状态，再决定要不要按切换键。
///
/// - 确认在播 → 按一次切换键（变为暂停）；
/// - 确认已暂停 → **不按**（否则会反向恢复播放）；
/// - 探测不可用 → 按一次切换键（尽力而为，对称处理）。
///
/// 返回暂停后的状态判定（`Paused` = 已确认暂停）。
pub fn ensure_paused(pid: i32, key_code: u16) -> PauseState {
    match is_playing(pid) {
        Some(true) => {
            let ok = toggle_playback_key(pid, key_code);
            if ok {
                tracing::info!(pid, event = "target_paused", "检测到目标在播放，已发送暂停键");
                PauseState::Paused
            } else {
                tracing::warn!(pid, event = "target_pause_failed", "暂停键发送失败，播放状态未知");
                PauseState::Unknown
            }
        }
        Some(false) => {
            tracing::debug!(pid, event = "target_pause_skip", "目标已处于暂停态，不重复发送切换键");
            PauseState::Paused
        }
        None => {
            // 探测不可用：尽力发一次（与恢复时动作对称）
            let ok = toggle_playback_key(pid, key_code);
            tracing::warn!(pid, ok, event = "target_pause_probe_unavailable", "播放状态探测不可用，已尽力发送暂停键");
            if ok {
                PauseState::Unknown
            } else {
                PauseState::Unknown
            }
        }
    }
}

// ---------- v3：播放状态探测（让空格 toggle 幂等化） ----------

/// 防绕过重暂停：仅当检测到目标**确实在播放**时，才补发一次暂停键。
///
/// 背景：空格是切换键——若视频本就暂停，再发一次反而会恢复播放。
/// 故守卫线程必须先判定播放状态再决定是否发送。探测不可用时**不发**
/// （宁可少一道防线，也不能反向恢复播放）。
///
/// 返回 true = 本次确实补发了暂停键。
pub fn repause_if_playing(pid: i32, key_code: u16) -> bool {
    match is_playing(pid) {
        Some(true) => {
            let ok = toggle_playback_key(pid, key_code);
            tracing::info!(pid, ok, event = "guard_repaused", "检测到目标仍在播放，补发暂停键");
            ok
        }
        Some(false) => {
            tracing::debug!(pid, event = "guard_repause_skip", "目标处于暂停态，无需补发");
            false
        }
        None => {
            tracing::warn!(pid, event = "guard_repause_skip", "播放状态探测不可用，本次不补发暂停键");
            false
        }
    }
}

/// 探测目标进程当前是否在解码播放（CPU 占用法）。
///
/// - `Some(true)`  = 正在播放（有解码开销）；
/// - `Some(false)` = 静止（已暂停）；
/// - `None`        = 探测不可用（调用方应保守处理）。
///
/// 平台差异：macOS 用 `ps -o time=` 的累计 CPU 时间差值；Windows 用 tasklist 的
/// CPU Time 列；Android 无法探测其他应用（恒 None）。
pub fn is_playing(pid: i32) -> Option<bool> {
    let first = cpu_time_sec(pid)?;
    std::thread::sleep(Duration::from_millis(CPU_SAMPLE_INTERVAL_MS));
    let second = cpu_time_sec(pid)?;
    Some((second - first) >= CPU_BUSY_THRESHOLD_SEC)
}

/// 采样窗口（毫秒）：两次累计 CPU 时间采样的间隔。
const CPU_SAMPLE_INTERVAL_MS: u64 = 1000;

/// 判定「正在解码」的累计 CPU 时间增量阈值（秒）。
///
/// **macOS 真机实测（哔哩哔哩 PC 客户端，2026-11-04，libproc 采样 1s 窗口）**：
/// - 首页稳定态（无人操作、无视频播放）：主进程 `0.040 / 0.040 / 0.040 / 0.119 / 0.072`
///   → 均值约 0.06，峰值 0.119；
/// - 冷启动加载期（启动后 8~20s）：主进程最高 `0.844`，整树 `1.143`；
/// - Electron 整树（主+直接子进程）稳定态：`0.167 ~ 0.278`。
///
/// 原阈值 0.12 贴着空闲峰值 0.119，几乎必然误判为「在播」——
/// 守卫若在视频已暂停时补发切换键，反而会**恢复播放**。故上调到 0.25，
/// 给空闲态留约 2× 余量；代价是冷启动加载期仍会被判为「在播」（方向安全：
/// 只是发一次暂停键，不会反向放行）。
///
/// ⚠️ 播放态（真正解码视频）的增量尚未实测（本机缺辅助功能权限、无法自动
/// 操作 B 站 UI）。家长真机验证时若发现「播放中但守卫不动作」，应下调此值。
const CPU_BUSY_THRESHOLD_SEC: f64 = 0.25;

/// 读取进程累计 CPU 时间（秒）。
///
/// ⚠️ 平台现实：部分 macOS 设备（企业策略 / 隐私限制）会把 `ps` 整体禁用
/// （`operation not permitted`），此时必须降级到 **libproc `proc_pidinfo`**
/// —— 纯用户态 API，不需要任何特权，也不受 `ps`/`top` 的策略限制。
/// 因此这里采用「ps 优先、libproc 兜底」的双路径。
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn cpu_time_sec(pid: i32) -> Option<f64> {
    let out = Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_cpu_time(&String::from_utf8_lossy(&out.stdout))
}

/// macOS：累计 CPU 时间（秒）。
///
/// 双路径：
/// 1. `ps -o time=` —— 标准路径，可用时最简单；
/// 2. `proc_pidinfo(PROC_PIDTASKINFO)` —— libproc 纯用户态 API，
///    在 `ps`/`top` 被系统策略禁用时仍可用（返回纳秒，精度更高）。
#[cfg(target_os = "macos")]
fn cpu_time_sec(pid: i32) -> Option<f64> {
    // ① ps 优先
    if let Some(sec) = cpu_time_sec_via_ps(pid) {
        return Some(sec);
    }
    // ② libproc 兜底
    cpu_time_sec_via_libproc(pid)
}

/// macOS：`ps -o time=` 路径（`ps` 被禁用时返回 None）。
#[cfg(target_os = "macos")]
fn cpu_time_sec_via_ps(pid: i32) -> Option<f64> {
    let out = Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_cpu_time(&String::from_utf8_lossy(&out.stdout))
}

/// macOS：libproc `proc_pidinfo(PROC_PIDTASKINFO)` 取累计 CPU 时间（秒）。
///
/// 纯用户态系统调用（libSystem 导出，无需 root / 无需辅助功能权限），
/// 不受 `ps`/`top` 被企业策略或隐私设置禁用的影响——这正是真机实测遇到的情况。
///
/// ⚠️ 两个实测得来的关键点：
/// 1. 内核实际写入 **96 字节**（不是 `struct proc_taskinfo` 头部的 48 字节），
///    缓冲区给小了 `proc_pidinfo` 会直接返回 0（读不到任何数据）；
/// 2. 累计 CPU 时间位于偏移 16 与 24（`pti_total_user` / `pti_total_system`，单位纳秒），
///    已用「自旋 1.5s → 增量 1.386s」实测校验精度。
#[cfg(target_os = "macos")]
fn cpu_time_sec_via_libproc(pid: i32) -> Option<f64> {
    use std::os::raw::c_void;

    /// `PROC_PIDTASKINFO` flavor 常量（libproc.h）。
    const PROC_PIDTASKINFO: i32 = 4;
    /// 内核实际写入长度（实测 96 字节），缓冲区必须 ≥ 此值，否则接口返回 0。
    const KERNEL_WRITE_BYTES: usize = 192;
    /// `pti_total_user` 在 u64 数组中的下标（偏移 16 字节）。
    const IDX_TOTAL_USER: usize = 2;
    /// `pti_total_system` 在 u64 数组中的下标（偏移 24 字节）。
    const IDX_TOTAL_SYSTEM: usize = 3;

    extern "C" {
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut c_void,
            buffersize: i32,
        ) -> i32;
    }

    // 对齐到 u64 的缓冲区
    let mut buf = [0u64; KERNEL_WRITE_BYTES / 8];
    let written = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDTASKINFO,
            0,
            buf.as_mut_ptr() as *mut c_void,
            (buf.len() * 8) as i32,
        )
    };
    if written <= 0 {
        // 进程不存在或无权读取
        return None;
    }
    let nanos = buf[IDX_TOTAL_USER].saturating_add(buf[IDX_TOTAL_SYSTEM]);
    Some(nanos as f64 / 1_000_000_000.0)
}

/// 解析 `ps -o time=` 输出：`[[dd-]hh:]mm:ss[.ss]` → 秒。
#[cfg(not(target_os = "windows"))]
fn parse_cpu_time(raw: &str) -> Option<f64> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    // 去掉 days 部分（形如 "2-03:04:05"）
    let (days, rest) = match text.split_once('-') {
        Some((d, r)) => (d.parse::<f64>().ok()?, r),
        None => (0.0, text),
    };
    let mut total = 0.0;
    for part in rest.split(':') {
        total = total * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(total + days * 86400.0)
}

/// Windows：tasklist 的 CPU Time 列（`h:mm:ss`）→ 累计 CPU 秒。
#[cfg(target_os = "windows")]
fn cpu_time_sec(pid: i32) -> Option<f64> {
    let filter = format!("PID eq {}", pid);
    let out = Command::new("tasklist")
        .args(["/FI", &filter, "/NH", "/FO", "CSV"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // CSV: "name.exe","pid","session","#","0:00:12"
    let line = text.lines().find(|l| l.contains(&pid.to_string()))?;
    let field = line.rsplit(',').next()?.trim().trim_matches('"');
    let mut total = 0.0;
    for part in field.split(':') {
        total = total * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(total)
}

/// 轮次剩余秒数：`round_started_epoch + use_minutes*60 - now`；≤0 表示到点。
///
/// - `use_minutes` 支持小数（U10 加速档：如 0.1 分钟 = 6 秒）
/// - 非法值（负数/NaN）按 0 秒轮次处理（立即到点，与 sanitize 下限语义一致）
pub fn remaining_sec(now: u64, round_started_epoch: u64, use_minutes: f64) -> i64 {
    let total_sec = if use_minutes.is_finite() {
        (use_minutes * 60.0).round() as i64
    } else {
        0
    };
    round_started_epoch as i64 + total_sec.max(0) - now as i64
}

/// 计算单轮时长（秒）：`fast_test=true` 固定 10 秒并忽略 use_minutes（U10 双轨）；
/// 否则 `use_minutes*60` 四舍五入；非法值按 0（sanitize 的下限兜底在 config 层）。
pub fn round_duration_sec(use_minutes: f64, fast_test: bool) -> i64 {
    if fast_test {
        return 10;
    }
    if use_minutes.is_finite() {
        (use_minutes * 60.0).round() as i64
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_pid_is_not_alive() {
        assert!(!alive(0));
        assert!(!alive(-1));
        // pid 99999999 在常规机器上不存在
        assert!(!alive(99_999_999));
    }

    #[test]
    fn remaining_sec_zero_minutes_expires_immediately() {
        assert_eq!(remaining_sec(1000, 1000, 0.0), 0);
        assert!(remaining_sec(1001, 1000, 0.0) < 0);
    }

    /// macOS：libproc 兜底必须能读到 CPU 时间（`ps` 被禁用时的唯一路径）。
    #[cfg(target_os = "macos")]
    #[test]
    fn libproc_reads_cpu_time_when_ps_is_restricted() {
        let pid = std::process::id() as i32;
        let t = cpu_time_sec_via_libproc(pid);
        assert!(
            t.is_some(),
            "libproc proc_pidinfo 应能读到本进程 CPU 时间（ps 受限时是唯一来源）"
        );
        assert!(t.unwrap() >= 0.0);
        // 完整读取链路也必须可用（ps 优先 → libproc 兜底）
        assert!(cpu_time_sec(pid).is_some());
    }

    /// macOS：`is_playing` 在「持续有 CPU 活动」的进程上应判定为「在播」。
    ///
    /// 用后台线程在 `is_playing` 的 1s 采样窗口内持续占用 CPU——
    /// 这与真实场景一致（B 站解码视频时主/渲染进程持续吃 CPU）。
    #[cfg(target_os = "macos")]
    #[test]
    fn is_playing_detects_busy_process() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let pid = std::process::id() as i32;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let busy = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                std::hint::black_box((0..100_000).sum::<u64>());
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        let r = is_playing(pid);
        stop.store(true, Ordering::Relaxed);
        let _ = busy.join();
        assert_eq!(r, Some(true), "忙进程应判定为在播放");
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn parse_cpu_time_handles_formats() {
        // mm:ss.ss
        assert!((parse_cpu_time("01:02.50").unwrap() - 62.5).abs() < 1e-6);
        // hh:mm:ss
        assert!((parse_cpu_time("01:02:03").unwrap() - 3723.0).abs() < 1e-6);
        // dd-hh:mm:ss
        assert!((parse_cpu_time("2-00:00:01").unwrap() - 172801.0).abs() < 1e-6);
        // 空串 → None
        assert!(parse_cpu_time("  ").is_none());
    }

    #[test]
    fn is_playing_returns_none_for_dead_pid() {
        // 不存在的进程：探测不可用 → None（调用方保守处理）
        assert!(is_playing(99_999_999).is_none());
    }
}
