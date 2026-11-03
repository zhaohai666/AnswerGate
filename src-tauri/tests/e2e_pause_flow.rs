//! v2.1 pause 模式端到端测试：超时暂停不退出程序，答题通过后原地恢复。
//!
//! 覆盖三层：
//! 1. 配置层：timeout_action / pause_key_code 解析与 sanitize；
//! 2. 状态机层：begin_timeup_paused 保留纳管字段 → PASS → extend_round 原地续期；
//! 3. 进程层（真实子进程）：pause_target 不杀进程；terminate 仍能真实杀死。

use answergate_lib::gate::{GatePhase, GateState, QuizReason};
use answergate_lib::model::{Level, TimeoutAction};
use answergate_lib::monitor;

use std::process::Command;
use std::time::Duration;

/// spawn 一个真实存活 60s 的子进程（孤儿化，模拟 launchd 托管的目标软件）。
/// 返回 `sleep` 本体的 pid（sh 包装进程立即退出，真实目标 pid 由 `$!` echo 取得）。
fn spawn_sleep_pid() -> i32 {
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
fn config_parses_pause_mode() {
    let text = r#"
        [monitor]
        timeout_action = "pause"
        pause_key_code = 205
        [[target_apps]]
        id = "bili"
        name = "哔哩哔哩"
        mac = "/Applications/BiliBili.app"
        use_minutes = 10
    "#;
    let cfg = answergate_lib::config::parse_config(text).expect("parse");
    assert_eq!(cfg.monitor.timeout_action, TimeoutAction::Pause);
    assert_eq!(cfg.monitor.pause_key_code, 205);
    assert_eq!(cfg.target_apps[0].use_minutes, 10.0);
}

#[test]
fn config_default_is_kill_and_key_code_49() {
    let cfg = answergate_lib::config::parse_config("").expect("parse");
    assert_eq!(cfg.monitor.timeout_action, TimeoutAction::Kill);
    assert_eq!(cfg.monitor.pause_key_code, 49);
}

#[test]
fn config_zero_key_code_sanitized_to_space() {
    let text = "[monitor]\npause_key_code = 0\n";
    let cfg = answergate_lib::config::parse_config(text).expect("parse");
    assert_eq!(cfg.monitor.pause_key_code, 49);
}

#[test]
fn pause_flow_keeps_managed_fields_and_resumes_in_place() {
    let mut gate = GateState::new(2);
    // 首答通过 → 纳管启动（pid=52050 模拟 Bilibili）
    gate.record_result(true, QuizReason::Initial, 120, 1000);
    gate.begin_using("bili", 52050, 1100, false).unwrap();
    assert_eq!(gate.phase, GatePhase::Using);
    assert!(!gate.paused);

    // 使用中到点：进入暂停答题（与 kill 模式的关键差异——纳管字段保留）
    gate.begin_timeup_paused(Level::Primary, Some("bili".into()), 1300)
        .unwrap();
    assert_eq!(gate.phase, GatePhase::Quiz);
    assert_eq!(gate.quiz_reason, QuizReason::Timeup);
    assert!(gate.paused);
    assert_eq!(gate.managed_pid, Some(52050), "pause 模式必须保留纳管 pid");
    assert_eq!(gate.managed_target_id.as_deref(), Some("bili"));

    // 暂停答题通过
    gate.record_result(true, QuizReason::Timeup, 120, 1400);
    assert_eq!(gate.phase, GatePhase::Passed);

    // 续期：同一个 pid 原地恢复（不重新拉起），paused 复位，新轮次开始
    gate.extend_round("bili", 52050, 1500).unwrap();
    assert_eq!(gate.phase, GatePhase::Using);
    assert!(!gate.paused);
    assert_eq!(gate.managed_pid, Some(52050));
    assert_eq!(gate.round_count, 2);
    assert_eq!(gate.round_started_epoch, Some(1500));
}

#[test]
fn pause_flow_persist_roundtrip_keeps_paused_flag() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".answergate_gate_state.json");
    let mut gate = GateState::new(2);
    gate.record_result(true, QuizReason::Initial, 120, 1000);
    gate.begin_using("bili", 1234, 1100, false).unwrap();
    gate.begin_timeup_paused(Level::Junior, Some("bili".into()), 1300)
        .unwrap();
    gate.save(&path).unwrap();

    let loaded = GateState::load(&path).unwrap().unwrap();
    assert!(loaded.paused, "paused 标志必须持久化（防重启丢失暂停语义）");
    assert_eq!(loaded.managed_pid, Some(1234));
    assert_eq!(loaded.quiz_reason, QuizReason::Timeup);
}

#[test]
fn real_process_pause_does_not_kill_and_terminate_still_works() {
    let pid = spawn_sleep_pid();
    // 等孤儿化稳定
    std::thread::sleep(Duration::from_millis(300));
    assert!(monitor::alive(pid), "子进程应存活");

    // pause：无论按键是否真正生效（辅助功能权限），进程都不允许被杀
    let key_sent = monitor::pause_target(pid, 49);
    assert!(monitor::alive(pid), "pause 模式禁止终止目标进程");
    tracing::info!(pid, key_sent, "pause_target 调用完成（key_sent 取决于辅助功能权限）");

    // kill 路径回归：terminate 仍能真实杀死
    assert!(monitor::terminate(pid), "terminate 应确认进程死亡");
    assert!(!monitor::alive(pid));
}

#[test]
fn resume_key_after_pause_process_stays_alive() {
    let pid = spawn_sleep_pid();
    std::thread::sleep(Duration::from_millis(300));
    let _ = monitor::pause_target(pid, 49);
    let _ = monitor::resume_target(pid, 49);
    assert!(monitor::alive(pid), "resume 同样不得终止目标进程");
    assert!(monitor::terminate(pid));
    assert!(!monitor::alive(pid));
}
