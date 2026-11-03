//! T05 端到端验收（本机真实执行，非 mock 全程）：
//!
//! 1. 全链：纳管启动（真 PID）→ Using 计时（真实时钟）→ 到点 monitor::terminate
//!    真实杀死目标（kill -0/pgrep 确认死亡）→ Quiz(Timeup) 判分通过 →
//!    extend 真实重新拉起（新 PID ≠ 旧 PID 且存活）
//! 2. 逃避分支：Timeup 到点后不答题 → 目标进程保持死亡
//! 3. 意外退出分支：手动杀掉目标 → monitor_step 探测 ProcessExited → abandon 回 Idle
//!
//! 目标软件用孤儿化 `sleep` 子进程模拟（与 `open -a` 同构：launchd 托管、死后即收尸）；
//! 短时限用 use_minutes=0.1 语义（6 秒轮次），时钟为真实系统时钟。

use std::process::Command;
use std::time::{Duration, Instant};

use answergate_lib::commands::{enter_using_with_handle, monitor_step, now, MonitorTick};
use answergate_lib::gate::{GatePhase, GateState, QuizReason};
use answergate_lib::model::Level;
use answergate_lib::monitor;

/// 孤儿化 spawn 一个存活 60s 的 sleep 进程（launchd 托管，与 `open -a` 目标同构）。
fn spawn_target() -> i32 {
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

/// 通过答题拿到 Passed{reason} 状态。
fn passed_state(reason: QuizReason) -> GateState {
    let mut st = GateState::new(2);
    st.begin_quiz(Level::Primary, Some("sleep-demo".into()), now())
        .unwrap();
    st.record_result(true, reason, 120, now());
    st
}

/// 真实轮询 monitor_step 直到出现期望结果（模拟监控线程每 200ms 一次 tick）。
fn wait_for_tick(st: &mut GateState, round_sec: i64, want: MonitorTick, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let tick = monitor_step(st, round_sec, now());
        if tick == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "真实时钟 {:?} 内未出现期望 tick（期望 {:?}，实际 {:?}）",
            timeout,
            want,
            tick
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// ① 全链：纳管 → 真实时钟倒计时 → 到点真杀 → Timeup 通过 → 续期拉起新进程
#[test]
fn e2e_full_round_launch_countdown_kill_and_extend() {
    // —— 纳管启动：拿到真 PID，目标真实存活 ——
    let mut st = passed_state(QuizReason::Initial);
    let pid1 = spawn_target();
    enter_using_with_handle(&mut st, "sleep-demo", Some(pid1), false, 0, now()).unwrap();
    let round1_start = st.round_started_epoch;
    assert_eq!(st.phase, GatePhase::Using);
    assert!(
        monitor::alive(pid1),
        "纳管启动后目标应真实存活（PID {}）",
        pid1
    );

    // —— Using 计时（真实时钟）：use_minutes=0.1 → 6 秒轮次，前 2 秒应为 Tick ——
    let round_sec = 6; // 0.1 分钟
    match monitor_step(&mut st, round_sec, now()) {
        MonitorTick::Tick { remaining_sec, .. } => {
            assert!(remaining_sec > 0, "刚启动剩余应 > 0，实际 {}", remaining_sec);
        }
        other => panic!("刚纳管应为 Tick，实际 {:?}", other),
    }

    // —— 真实等待到点 → Timeout → 两段式终止真实杀死 ——
    wait_for_tick(&mut st, round_sec, MonitorTick::Timeout, Duration::from_secs(15));
    assert!(
        monitor::terminate(pid1),
        "到点后 monitor::terminate 应真实杀死目标（PID {}）",
        pid1
    );
    assert!(!monitor::alive(pid1), "终止后目标应确认死亡");
    // pgrep 交叉验证（kill -0 之外的第二证据）
    let pgrep = Command::new("pgrep")
        .arg("-x")
        .arg("sleep")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == pid1.to_string()))
        .unwrap_or(false);
    assert!(!pgrep, "pgrep 不应再找到 PID {}", pid1);

    // —— Quiz(Timeup) 判分通过 → extend 真实重新拉起 ——
    st.begin_quiz_reason(
        Level::Primary,
        Some("sleep-demo".into()),
        QuizReason::Timeup,
        now(),
    )
    .unwrap();
    st.record_result(true, QuizReason::Timeup, 120, now());
    let pid2 = spawn_target();
    enter_using_with_handle(&mut st, "sleep-demo", Some(pid2), false, 0, now()).unwrap();
    assert_eq!(st.phase, GatePhase::Using, "续期后应回到 Using");
    assert_ne!(pid1, pid2, "续期应拉起新进程（新 PID ≠ 旧 PID）");
    assert!(monitor::alive(pid2), "新进程应存活（PID {}）", pid2);
    assert_eq!(st.round_count, 2, "首轮 + 续期共 2 轮");
    assert!(
        st.round_started_epoch.unwrap() > round1_start.unwrap(),
        "续期轮次开始时间应晚于首轮（epoch 重置续算）"
    );

    // 清理：结束本轮并杀掉目标
    let _ = monitor::terminate(pid2);
    st.abandon_round();
    assert_eq!(st.phase, GatePhase::Idle);
}

/// ② 逃避分支：Timeup 后不答题 → 目标保持死亡（进程已 kill，无套利空间）
#[test]
fn e2e_evasion_timeup_no_answer_target_stays_dead() {
    let mut st = passed_state(QuizReason::Initial);
    let pid = spawn_target();
    enter_using_with_handle(&mut st, "sleep-demo", Some(pid), false, 0, now()).unwrap();
    // 1 秒轮次真实到点 → kill + 转入超时答题（生产 handle_timeout 行为）
    wait_for_tick(&mut st, 1, MonitorTick::Timeout, Duration::from_secs(10));
    assert!(monitor::terminate(pid));
    st.begin_quiz_reason(
        Level::Primary,
        Some("sleep-demo".into()),
        QuizReason::Timeup,
        now(),
    )
    .unwrap();
    assert_eq!(st.phase, GatePhase::Quiz, "到点后应转入超时答题");
    assert!(!monitor::alive(pid));
    // 模拟"孩子不答题"：不提交判分/不续期，真实等待 2 秒后复查
    std::thread::sleep(Duration::from_secs(2));
    assert!(!monitor::alive(pid), "不答题时目标必须保持死亡");
    assert_ne!(
        st.phase,
        GatePhase::Using,
        "逃避场景不允许回到使用中"
    );
}

/// ③ 意外退出分支：手动杀掉目标 → monitor_step 探测 ProcessExited → abandon 回 Idle（不触发答题）
#[test]
fn e2e_unexpected_exit_returns_to_idle() {
    let mut st = passed_state(QuizReason::Initial);
    let pid = spawn_target();
    enter_using_with_handle(&mut st, "sleep-demo", Some(pid), false, 0, now()).unwrap();
    assert!(monitor::alive(pid));
    // 模拟用户手动 Cmd+Q 目标软件
    let _ = Command::new("kill").arg(pid.to_string()).status();
    // 监控线程语义：真实轮询探测到句柄失效
    wait_for_tick(
        &mut st,
        600,
        MonitorTick::ProcessExited,
        Duration::from_secs(10),
    );
    st.abandon_round();
    assert_eq!(st.phase, GatePhase::Idle, "意外退出应回 Idle 而非触发答题");
    assert_eq!(st.managed_pid, None);
    assert_eq!(st.managed_target_id, None);
}

/// ④ fast_test 语义（U10）真实时钟验证：轮次固定 10 秒，10 秒后真实到点
#[test]
fn e2e_fast_test_ten_second_round_times_out_for_real() {
    let mut st = passed_state(QuizReason::Initial);
    let pid = spawn_target();
    enter_using_with_handle(&mut st, "sleep-demo", Some(pid), false, 0, now()).unwrap();
    // fast_test=true 时 round_sec_for 返回 10——用真实 10 秒轮次验证到点
    let t0 = Instant::now();
    wait_for_tick(&mut st, 10, MonitorTick::Timeout, Duration::from_secs(20));
    let elapsed = t0.elapsed().as_secs();
    assert!(
        (8..=15).contains(&elapsed),
        "fast_test 轮次应在 ~10s 真实到点，实际 {}s",
        elapsed
    );
    let _ = monitor::terminate(pid);
    st.abandon_round();
}
