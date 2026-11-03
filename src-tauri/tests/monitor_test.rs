//! T01 进程句柄竖切集成测试：
//! 1. 真实子进程（`sleep 60`）的 alive / terminate 链路（macOS 本机可验证）
//! 2. remaining_sec 纯函数边界：0.1 分钟小数轮次、恰好到点、已超时为负
//! 3. MockClock 注入后驱动 now() 与 remaining_sec 组合
//! 4. 取不到 PID 的路径：不存在的 app 名 → None（U7 宁不管控不误杀）
//! 5. [monitor] fast_test 解析预留（U10）
//! 6. 真实系统应用（预览）端到端：open → 取 PID → 两段式 kill（#[ignore]，需真机跑）
//!
//! 运行：`cargo test`（常规）；`cargo test --test monitor_test -- --ignored`（真实 App 用例）。

use std::process::Command;

use answergate_lib::clock::{Clock, MockClock, SystemClock};
use answergate_lib::config;
use answergate_lib::monitor::{alive, remaining_sec, terminate, ManagedProcess};

/// spawn 一个存活 60s 的 sleep 进程并返回 pid。
///
/// 经 `sh -c 'sleep 60 >/dev/null 2>&1 & echo $!'` 孤儿化：sh 立即退出，sleep 被
/// 转交 launchd 托管——与 `open -a` 拉起的真实目标同构（死后由 launchd 收尸，
/// `kill -0` 不会误判僵尸态）；sleep 输出重定向 /dev/null 避免管道占用阻塞 output()。
fn spawn_sleep_60() -> i32 {
    let out = Command::new("sh")
        .arg("-c")
        .arg("sleep 60 >/dev/null 2>&1 & echo $!")
        .output()
        .expect("本机应可 spawn sh");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<i32>()
        .expect("应输出 sleep 子进程 pid")
}

#[test]
fn spawned_process_alive_then_terminated() {
    let pid = spawn_sleep_60();
    assert!(alive(pid), "刚 spawn 的 sleep 进程应存活");
    assert!(terminate(pid), "两段式终止应确认进程死亡");
    assert!(!alive(pid), "终止后存活检查应为 false");
}

#[test]
fn managed_process_tracks_lifecycle() {
    let pid = spawn_sleep_60();
    let proc = ManagedProcess::new(pid, "测试睡眠", "sleep 60", 1_700_000_000);
    assert_eq!(proc.pid, pid);
    assert_eq!(proc.target_name, "测试睡眠");
    assert_eq!(proc.round_started_epoch, 1_700_000_000);
    assert!(proc.is_alive());
    assert!(proc.terminate());
    assert!(!proc.is_alive());
}

#[test]
fn remaining_sec_boundaries() {
    let start: u64 = 1_000_000;
    // 0.1 分钟 = 6 秒（U10 小数轮次：fast_test 加速档可用）
    assert_eq!(remaining_sec(start, start, 0.1), 6);
    assert_eq!(remaining_sec(start + 5, start, 0.1), 1);
    assert_eq!(remaining_sec(start + 6, start, 0.1), 0, "恰好到点为 0");
    assert!(remaining_sec(start + 7, start, 0.1) < 0, "已超时为负");
    // 标准轮次：10 分钟
    assert_eq!(remaining_sec(start, start, 10.0), 600);
    // 非法值按 0 秒轮次（立即到点，与 sanitize 下限语义一致）
    assert_eq!(remaining_sec(start, start, -1.0), 0);
    assert_eq!(remaining_sec(start, start, f64::NAN), 0);
}

#[test]
fn mock_clock_drives_remaining_sec() {
    let clock = MockClock::new(1_700_000_000);
    assert_eq!(clock.now(), 1_700_000_000);
    let start = clock.now();
    // 6 秒轮次未到点
    clock.advance(5);
    assert_eq!(remaining_sec(clock.now(), start, 0.1), 1);
    // 恰好走完
    clock.advance(1);
    assert_eq!(remaining_sec(clock.now(), start, 0.1), 0);
}

#[test]
fn system_clock_returns_plausible_epoch_seconds() {
    // 2020-01-01 之后的合理值
    assert!(SystemClock.now() > 1_577_836_800);
}

#[cfg(target_os = "macos")]
#[test]
fn pid_of_missing_app_returns_none() {
    use answergate_lib::unlock;
    // 不存在的 app 名：osascript 与 pgrep 双双失败，5s 窗口耗尽后返回 None（U7）
    let pid = unlock::pid_of_app("/Applications/NoSuchApp_AnswerGate_XYZ.app");
    assert!(pid.is_none());
}

#[test]
fn monitor_fast_test_config_parsed() {
    // [monitor] 段预留 fast_test（U10）
    let cfg = config::parse_config("[monitor]\nfast_test = true\n").unwrap();
    assert!(cfg.monitor.fast_test);
    // 缺段回退 false
    let cfg = config::parse_config("").unwrap();
    assert!(!cfg.monitor.fast_test);
}

/// 真实系统应用端到端（T01 完成标准）：open 预览 → osascript 取到 PID →
/// kill -0 存活为真 → 两段式 kill 真实关闭。
#[cfg(target_os = "macos")]
#[test]
#[ignore] // 需真实桌面环境：cargo test --test monitor_test -- --ignored
fn real_preview_app_pid_acquired_and_terminated() {
    use answergate_lib::unlock;
    let preview = "/System/Applications/Preview.app";
    if !std::path::Path::new(preview).exists() {
        // 极简环境无预览则跳过（不断言失败）
        return;
    }
    let pid = unlock::launch_macos_managed(preview)
        .expect("open 预览应成功")
        .expect("osascript 应取得预览 PID");
    assert!(pid > 0);
    assert!(alive(pid), "刚取得的 PID 应存活");
    assert!(terminate(pid), "两段式终止应确认预览已关闭");
    assert!(!alive(pid));
}
