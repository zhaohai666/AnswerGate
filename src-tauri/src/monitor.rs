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
}
