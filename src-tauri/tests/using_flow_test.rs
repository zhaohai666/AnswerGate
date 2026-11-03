//! v2 T02/T03 集成测试：Using 轮次制状态机 + [monitor] 配置 + 监控单步逻辑。
//!
//! 覆盖：
//! 1. Using 进入（begin_using / extend_round）/ 放弃（abandon_round）/ 轮次上限
//! 2. 句柄结果分流：Some(pid) 入 Using、None → abandon + MonitorFailed（U7）
//! 3. monitor_step：计时中 / 到点 Timeout / 进程死亡 ProcessExited / 非 Using Stopped
//! 4. 重启续算：save → recover → round_remaining_sec 续算；句柄失效回 Idle（AppState 全链路）
//! 5. fast_test（U10）：固定 10 秒；use_minutes sanitize 下限 0.1

use answergate_lib::commands::{
    enter_using_with_handle, monitor_step, round_sec_for, MonitorTick, MonitorStateView,
};
use answergate_lib::config;
use answergate_lib::error::AppError;
use answergate_lib::gate::{GatePhase, GateState, QuizReason};
use answergate_lib::model::Level;

const NOW: i64 = 2_000_000_000;
const COOLDOWN: u64 = 120;

/// 造一个 Passed{Initial} 状态（可直接 begin_using）。
fn passed_state() -> GateState {
    let mut st = GateState::new(2);
    st.begin_quiz(Level::Primary, Some("demo-notes".into()), NOW).unwrap();
    st.record_result(true, QuizReason::Initial, COOLDOWN, NOW);
    assert_eq!(st.phase, GatePhase::Passed);
    st
}

/// 造一个 Passed{Timeup} 状态（可 extend_round）。
fn passed_timeup_state() -> GateState {
    let mut st = GateState::new(2);
    st.begin_quiz(Level::Primary, Some("demo-notes".into()), NOW).unwrap();
    st.record_result(true, QuizReason::Timeup, COOLDOWN, NOW);
    assert_eq!(st.phase, GatePhase::Passed);
    assert_eq!(st.quiz_reason, QuizReason::Timeup);
    st
}

// ---------- 1. Using 转移 ----------

#[test]
fn begin_using_enters_using_with_round_fields() {
    let mut st = passed_state();
    st.begin_using("demo-notes", 4321, NOW + 1, false).unwrap();
    assert_eq!(st.phase, GatePhase::Using);
    assert_eq!(st.round_count, 1);
    assert_eq!(st.round_started_epoch, Some(NOW + 1));
    assert_eq!(st.managed_pid, Some(4321));
    assert_eq!(st.managed_target_id, Some("demo-notes".into()));
    // 使用中重复纳管 → AlreadyManaged
    assert!(matches!(
        st.begin_using("demo-notes", 4322, NOW + 2, false),
        Err(AppError::AlreadyManaged)
    ));
    // 使用中直接开新答题（非 Timeup）→ 拒绝
    assert!(st
        .begin_quiz(Level::Primary, None, NOW + 2)
        .is_err());
}

#[test]
fn abandon_round_returns_idle_without_quiz() {
    let mut st = passed_state();
    st.begin_using("demo-notes", 4321, NOW, false).unwrap();
    st.abandon_round();
    assert_eq!(st.phase, GatePhase::Idle, "提前结束不进 QUIZ");
    assert_eq!(st.round_started_epoch, None);
    assert_eq!(st.managed_pid, None);
    assert_eq!(st.managed_target_id, None);
    // Passed{Initial} 上 abandon 亦回 Idle（取句柄失败路径）
    let mut st2 = passed_state();
    st2.abandon_round();
    assert_eq!(st2.phase, GatePhase::Idle);
}

#[test]
fn extend_round_renews_epoch_and_counts() {
    let mut st = passed_timeup_state();
    st.extend_round("demo-notes", 4321, NOW + 100).unwrap();
    assert_eq!(st.phase, GatePhase::Using);
    assert_eq!(st.round_count, 1);
    assert_eq!(st.round_started_epoch, Some(NOW + 100));
    // 非 Passed{Timeup} 不可续期
    let mut st2 = passed_state();
    assert!(st2.extend_round("demo-notes", 4321, NOW).is_err());
}

#[test]
fn handle_none_abandons_and_reports_monitor_failed() {
    let mut st = passed_state();
    let err = enter_using_with_handle(&mut st, "demo-notes", None, false, 0, NOW).unwrap_err();
    assert_eq!(err.code(), "MonitorFailed");
    assert_eq!(st.phase, GatePhase::Idle, "取不到句柄 → abandon 回 Idle（U7）");
}

#[test]
fn handle_some_routes_by_quiz_reason() {
    // Initial → begin_using
    let mut st = passed_state();
    enter_using_with_handle(&mut st, "demo-notes", Some(111), false, 0, NOW).unwrap();
    assert_eq!(st.phase, GatePhase::Using);
    assert_eq!(st.managed_pid, Some(111));
    // Timeup → extend_round
    let mut st2 = passed_timeup_state();
    enter_using_with_handle(&mut st2, "demo-notes", Some(222), false, 0, NOW + 50).unwrap();
    assert_eq!(st2.phase, GatePhase::Using);
    assert_eq!(st2.round_started_epoch, Some(NOW + 50));
}

#[test]
fn round_limit_reached_blocks_extension() {
    let mut st = passed_timeup_state();
    st.round_count = 3; // 已用 3 轮
    let err =
        enter_using_with_handle(&mut st, "demo-notes", Some(333), false, 3, NOW).unwrap_err();
    assert_eq!(err.code(), "RoundLimitReached");
    assert_eq!(st.phase, GatePhase::Passed, "上限拒绝不改变状态");
    // force（家长应急）不受限
    enter_using_with_handle(&mut st, "demo-notes", Some(333), true, 3, NOW).unwrap();
    assert_eq!(st.phase, GatePhase::Using);
}

// ---------- 2. monitor_step ----------

/// 孤儿化 spawn 一个 sleep 进程（与 `open -a` 同构：死后 launchd 收尸）。
fn spawn_sleep_60() -> i32 {
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg("sleep 60 >/dev/null 2>&1 & echo $!")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<i32>()
        .unwrap()
}

#[test]
fn monitor_step_paths() {
    // 非 Using → Stopped
    let mut idle = GateState::new(2);
    assert_eq!(monitor_step(&mut idle, 600, NOW), MonitorTick::Stopped);

    // Using + 不存在的 pid → ProcessExited
    let mut st = passed_state();
    st.begin_using("demo-notes", 99_999_999, NOW, false).unwrap();
    assert_eq!(
        monitor_step(&mut st, 600, NOW + 1),
        MonitorTick::ProcessExited
    );

    // Using + 存活 pid + 未到点 → Tick（剩余 > 0）
    let pid = spawn_sleep_60();
    let mut live = passed_state();
    live.begin_using("demo-notes", pid, NOW, false).unwrap();
    match monitor_step(&mut live, 600, NOW + 5) {
        MonitorTick::Tick {
            remaining_sec,
            warning,
        } => {
            assert_eq!(remaining_sec, 595);
            assert!(!warning, "剩余远大于 60s 不告警");
        }
        other => panic!("期望 Tick，实际 {:?}", other),
    }
    // 剩余 ≤ 60s → warning
    match monitor_step(&mut live, 600, NOW + 550) {
        MonitorTick::Tick { remaining_sec, warning } => {
            assert_eq!(remaining_sec, 50);
            assert!(warning);
        }
        other => panic!("期望 warning Tick，实际 {:?}", other),
    }
    // 恰好到点 → Timeout
    assert_eq!(monitor_step(&mut live, 600, NOW + 600), MonitorTick::Timeout);
    // 已超时（负数）→ Timeout
    assert_eq!(monitor_step(&mut live, 600, NOW + 700), MonitorTick::Timeout);
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
}

#[test]
fn fast_test_round_is_fixed_10_seconds() {
    // U10：fast_test=true 固定 10 秒，忽略 use_minutes
    assert_eq!(answergate_lib::monitor::round_duration_sec(600.0, true), 10);
    assert_eq!(answergate_lib::monitor::round_duration_sec(10.0, false), 600);
    assert_eq!(answergate_lib::monitor::round_duration_sec(0.1, false), 6);
}

// ---------- 3. 重启续算 ----------

#[test]
fn round_remaining_survives_restart_via_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let mut st = passed_state();
    st.begin_using("demo-notes", 4321, NOW, false).unwrap();
    st.save(&path).unwrap();
    // 重启 5s 后恢复：轮次 10 分钟 → 剩余 595，仍 Using（续算，不重置）
    let recovered = GateState::recover(&path, 2, NOW + 5);
    assert_eq!(recovered.phase, GatePhase::Using);
    assert_eq!(recovered.round_remaining_sec(NOW + 5, 600), Some(595));
    assert_eq!(recovered.round_started_epoch, Some(NOW));
    // 已超时 → 装配层判 remaining<=0 后 abandon 回 Idle
    let mut expired = GateState::recover(&path, 2, NOW + 700);
    assert!(expired.round_remaining_sec(NOW + 700, 600).unwrap_or(0) <= 0);
    expired.abandon_round();
    assert_eq!(expired.phase, GatePhase::Idle);
}

#[test]
fn startup_revalidation_abandons_when_handle_dead() {
    // AppState 全链路：Using 状态 + 已死 pid → AppState::new 恢复后应回 Idle
    let dir = tempfile::tempdir().unwrap();
    let bank_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../resources/question_bank");
    let mut st = passed_state();
    st.begin_using("demo-notes", 99_999_999, NOW, false).unwrap();
    st.save(&dir.path().join(".answergate_gate_state.json")).unwrap();
    let app = answergate_lib::commands::AppState::new(dir.path().to_path_buf(), &bank_dir).unwrap();
    assert!(
        !answergate_lib::commands::revalidate_using_on_startup(
            &app,
            answergate_lib::commands::now()
        ),
        "pid 已死应重验失败"
    );
    assert_eq!(app.gate.lock().unwrap().phase, GatePhase::Idle);
}

#[test]
fn startup_revalidation_keeps_using_when_alive_and_time_left() {
    let dir = tempfile::tempdir().unwrap();
    let bank_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../resources/question_bank");
    let pid = spawn_sleep_60();
    let mut st = passed_state();
    let started = answergate_lib::commands::now();
    st.begin_using("demo-notes", pid, started, false).unwrap();
    st.save(&dir.path().join(".answergate_gate_state.json")).unwrap();
    let app = answergate_lib::commands::AppState::new(dir.path().to_path_buf(), &bank_dir).unwrap();
    assert!(
        answergate_lib::commands::revalidate_using_on_startup(&app, answergate_lib::commands::now()),
        "存活 pid + 默认 10 分钟轮次应恢复纳管"
    );
    assert_eq!(app.gate.lock().unwrap().phase, GatePhase::Using);
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
}

// ---------- 4. [monitor] 配置 ----------

#[test]
fn monitor_config_full_fields_parsed() {
    let cfg = config::parse_config(
        "[monitor]\nfast_test = true\nmin_use_minutes = 0.5\nmax_rounds = 3\n",
    )
    .unwrap();
    assert!(cfg.monitor.fast_test);
    assert_eq!(cfg.monitor.min_use_minutes, 0.5);
    assert_eq!(cfg.monitor.max_rounds, 3);
    // 缺省回退
    let cfg = config::parse_config("").unwrap();
    assert!(!cfg.monitor.fast_test);
    assert_eq!(cfg.monitor.min_use_minutes, 0.1);
    assert_eq!(cfg.monitor.max_rounds, 0);
}

#[test]
fn use_minutes_sanitized_to_floor() {
    // 低于下限 → 回退 0.1（架构 #4）
    let cfg = config::parse_config(
        "[[target_apps]]\nid = \"a\"\nname = \"A\"\nuse_minutes = 0.01\n",
    )
    .unwrap();
    assert_eq!(cfg.target_apps[0].use_minutes, 0.1);
    // 负数/NaN → 回退下限
    let cfg = config::parse_config(
        "[[target_apps]]\nid = \"a\"\nname = \"A\"\nuse_minutes = -5\n",
    )
    .unwrap();
    assert_eq!(cfg.target_apps[0].use_minutes, 0.1);
    // 缺字段 → 默认 10 分钟
    let cfg = config::parse_config("[[target_apps]]\nid = \"a\"\nname = \"A\"\n").unwrap();
    assert_eq!(cfg.target_apps[0].use_minutes, 10.0);
}

#[test]
fn round_sec_uses_target_use_minutes() {
    let cfg = config::parse_config(
        "[monitor]\nfast_test = true\n[[target_apps]]\nid = \"a\"\nname = \"A\"\nuse_minutes = 5\n",
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let bank_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../resources/question_bank");
    std::fs::write(dir.path().join("config.toml"), toml::to_string(&cfg).unwrap()).unwrap();
    let st = answergate_lib::commands::AppState::new(dir.path().to_path_buf(), &bank_dir).unwrap();
    // fast_test=true → 固定 10 秒
    assert_eq!(round_sec_for(&st, Some("a")), 10);
}

#[test]
fn monitor_state_view_serializes() {
    // 视图字段完整性（前端契约）
    let view = MonitorStateView {
        phase: GatePhase::Using,
        managed_target_name: Some("备忘录演示".into()),
        remaining_sec: 595,
        rounds_used: 1,
        warning: false,
    };
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["phase"], "using");
    assert_eq!(json["remaining_sec"], 595);
    assert_eq!(json["rounds_used"], 1);
}
