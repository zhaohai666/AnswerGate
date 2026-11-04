//! v3 独立进程监控（watchdog）端到端测试。
//!
//! 覆盖需求：
//! 1. 孩子绕过门禁自己打开 B 站 → watchdog 必须发现并拦截（要求答题）；
//! 2. 答对后接管**既有 PID**（不新拉起），并开始 10 分钟倒计时；
//! 3. 超时/监控拦截的续期卷必须「3 题全对」；
//! 4. pause 模式「只暂停，不能删除」——冷却期也不得终止进程。

use answergate_lib::commands::{self, AppState, WatchDecision};
use answergate_lib::gate::{GatePhase, GateState, QuizReason};
use answergate_lib::model::{Level, PauseState, TimeoutAction};

use std::path::PathBuf;

// ---------- 1. watchdog 判定 ----------

#[test]
fn watch_decision_intercepts_when_target_started_outside_answergate() {
    let gate = GateState::new(2);
    // 目标在跑、本程序没纳管、门禁空闲 → 必须拦截
    assert_eq!(
        commands::watch_decision(&gate, "bili", None, true),
        WatchDecision::Intercept
    );
}

#[test]
fn watch_decision_ignores_not_running_target() {
    let gate = GateState::new(2);
    assert_eq!(
        commands::watch_decision(&gate, "bili", None, false),
        WatchDecision::None
    );
}

#[test]
fn watch_decision_ignores_managed_target() {
    // 已由本程序纳管的目标由 using monitor 计时，不得重复拦截
    let gate = GateState::new(2);
    assert_eq!(
        commands::watch_decision(&gate, "bili", Some("bili"), true),
        WatchDecision::None
    );
}

#[test]
fn watch_decision_ignores_non_idle_phase() {
    // 答题中/纳管中/冷却中都不抢状态
    let mut gate = GateState::new(2);
    gate.record_result(true, QuizReason::Initial, 120, 1000);
    gate.begin_using("bili", 111, 1100, false).unwrap();
    assert_eq!(
        commands::watch_decision(&gate, "other", None, true),
        WatchDecision::None
    );
}

#[test]
fn watch_decision_ignores_already_intercepted() {
    let mut gate = GateState::new(2);
    gate.begin_watch_intercept(Level::Primary, Some("bili".into()), 222, 1000, PauseState::Paused)
        .unwrap();
    assert_eq!(
        commands::watch_decision(&gate, "bili", None, true),
        WatchDecision::None,
        "拦截期间不得重复触发"
    );
}

// ---------- 2. 拦截 → 答对 → 接管既有进程 ----------

#[test]
fn watch_intercept_then_adopt_keeps_existing_pid() {
    let mut gate = GateState::new(2);
    // watchdog 拦截：目标进程 54321 是孩子自己开的
    gate.begin_watch_intercept(Level::Primary, Some("bili".into()), 54321, 1000, PauseState::Paused)
        .unwrap();
    assert_eq!(gate.phase, GatePhase::Quiz);
    assert_eq!(gate.quiz_reason, QuizReason::Watch);
    assert!(gate.watch_intercepted);
    assert_eq!(gate.watch_pid, Some(54321));
    assert_eq!(gate.pause_state, PauseState::Paused);

    // 答对
    gate.record_result(true, QuizReason::Watch, 120, 1100);
    assert_eq!(gate.phase, GatePhase::Passed);
    assert_eq!(gate.quiz_reason, QuizReason::Watch);

    // 接管既有 PID：不得新拉起进程
    gate.adopt_running("bili", 54321, 1200, false).unwrap();
    assert_eq!(gate.phase, GatePhase::Using);
    assert_eq!(gate.managed_pid, Some(54321), "必须接管既有 pid，而非拉起新的");
    assert_eq!(gate.managed_target_id.as_deref(), Some("bili"));
    assert!(!gate.watch_intercepted);
    assert_eq!(gate.watch_pid, None);
    assert_eq!(gate.round_count, 1);
}

#[test]
fn adopt_running_rejects_unrelated_phase() {
    // 非监控拦截场景不允许接管（force=false）
    let mut gate = GateState::new(2);
    gate.record_result(true, QuizReason::Initial, 120, 1000);
    assert!(gate.adopt_running("bili", 1, 1100, false).is_err());
    // force=true（WatchMode::Adopt 路径）允许直接接管
    assert!(gate.adopt_running("bili", 1, 1100, true).is_ok());
}

#[test]
fn abandon_watch_intercept_resets_to_idle_and_clears_flags() {
    let mut gate = GateState::new(2);
    gate.begin_watch_intercept(Level::Primary, Some("bili".into()), 333, 1000, PauseState::Paused)
        .unwrap();
    gate.abandon_watch_intercept();
    assert_eq!(gate.phase, GatePhase::Idle);
    assert!(!gate.watch_intercepted);
    assert_eq!(gate.watch_pid, None);
    // 放弃后必须能再次拦截（否则 watchdog 永久失效）
    assert_eq!(
        commands::watch_decision(&gate, "bili", None, true),
        WatchDecision::Intercept
    );
}

#[test]
fn cooldown_expiry_clears_watch_flags_so_watchdog_can_reintercept() {
    // 关键防死锁：冷却到点复位到 Idle 时必须清掉 watch_intercepted，
    // 否则 watchdog 永远认为「已拦截过」，孩子再开 B 站就无人管。
    let mut gate = GateState::new(0);
    gate.begin_watch_intercept(Level::Primary, Some("bili".into()), 444, 1000, PauseState::Paused)
        .unwrap();
    // 全错且无重试 → 冷却
    gate.record_result(false, QuizReason::Watch, 60, 1100);
    assert_eq!(gate.phase, GatePhase::Cooldown);
    assert!(gate.watch_intercepted);

    // 冷却到点
    assert!(gate.finish_cooldown_if_elapsed(1200));
    assert_eq!(gate.phase, GatePhase::Idle);
    assert!(!gate.watch_intercepted, "冷却复位必须清拦截标记");
    assert_eq!(gate.watch_pid, None);
    assert_eq!(
        commands::watch_decision(&gate, "bili", None, true),
        WatchDecision::Intercept,
        "冷却结束后应能再次拦截仍在运行的 B 站"
    );
}

// ---------- 3. 暂停守卫覆盖监控拦截场景 ----------

#[test]
fn paused_guard_covers_watch_intercept_scenario() {
    let mut gate = GateState::new(2);
    gate.begin_watch_intercept(Level::Primary, Some("bili".into()), 555, 1000, PauseState::Paused)
        .unwrap();
    // 拦截答题期间：孩子切回 B 站点播放，守卫必须继续压制
    assert!(commands::paused_guard_step(&gate, 555));
    assert!(
        !commands::paused_guard_step(&gate, 666),
        "pid 不匹配不应触发"
    );

    // 答对并接管 → 进入 Using → 守卫退出
    gate.record_result(true, QuizReason::Watch, 120, 1100);
    gate.adopt_running("bili", 555, 1200, false).unwrap();
    assert!(!commands::paused_guard_step(&gate, 555));
}

// ---------- 4. 严格档门槛（3 题全对） ----------

fn fixture_state(cfg_extra: &str) -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let bank_dir = dir.path().join("bank");
    std::fs::create_dir_all(&bank_dir).unwrap();
    for (level, prefix) in [("kindergarten", "k"), ("primary", "p"), ("junior", "j")] {
        // 每个学段 5 题（够抽常规卷 5 题与严格档卷 3 题）
        let qs: Vec<serde_json::Value> = (0..5)
            .map(|i| {
                serde_json::json!({
                    "id": format!("{}_{}", prefix, i), "level": level, "grade": null,
                    "subject": "math", "type": "single_choice", "title": "占位",
                    "options": ["A", "B"], "answer": "A", "hint": null
                })
            })
            .collect();
        std::fs::write(
            bank_dir.join(format!("{}.json", level)),
            serde_json::to_string(&qs).unwrap(),
        )
        .unwrap();
    }
    let cfg = format!(
        "[quiz]\nquestion_count = 5\npass_score = 0.8\nmax_retry = 2\n\
         [monitor]\ntimeout_action = \"pause\"\n{}\n\
         [[target_apps]]\nid = \"bili\"\nname = \"哔哩哔哩\"\nmac = \"/Applications/bilibili.app\"\nuse_minutes = 10\nwatch = true\n",
        cfg_extra
    );
    std::fs::write(dir.path().join("config.toml"), cfg).unwrap();
    let state = AppState::new(dir.path().to_path_buf(), &bank_dir).unwrap();
    (dir, state)
}

#[test]
fn timeup_and_watch_use_strict_three_questions_all_correct() {
    let (_dir, state) = fixture_state("");
    // 超时续期：3 题 + 全对
    assert_eq!(
        commands::paper_count_for(&state, QuizReason::Timeup),
        3,
        "超时续期必须是 3 题"
    );
    assert_eq!(commands::pass_score_for(&state, QuizReason::Timeup), 1.0);
    // 监控拦截：同样严格档
    assert_eq!(
        commands::paper_count_for(&state, QuizReason::Watch),
        3,
        "监控拦截必须是 3 题"
    );
    assert_eq!(commands::pass_score_for(&state, QuizReason::Watch), 1.0);
    // 首次解锁：沿用常规配置
    assert_eq!(
        commands::paper_count_for(&state, QuizReason::Initial),
        5
    );
    assert_eq!(commands::pass_score_for(&state, QuizReason::Initial), 0.8);
}

#[test]
fn strict_threshold_config_is_sanitized() {
    // timeup_question_count = 0 会导致永远抽不出题（卡死）→ 必须回退 3
    let (_dir, state) = fixture_state("timeup_question_count = 0\ntimeup_pass_score = 0.2\n");
    assert_eq!(state.config.monitor.timeup_question_count, 3);
    assert_eq!(state.config.monitor.timeup_pass_score, 1.0);
}

#[test]
fn config_defaults_enable_watchdog_with_three_second_interval() {
    let cfg = answergate_lib::config::parse_config("").expect("parse");
    assert!(cfg.monitor.watch_enabled);
    assert_eq!(cfg.monitor.watch_interval_sec, 3);
    assert_eq!(cfg.monitor.timeup_question_count, 3);
    assert_eq!(cfg.monitor.timeup_pass_score, 1.0);
    assert!(
        !cfg.monitor.pause_fallback_kill,
        "v3：默认不得回退 kill（只暂停，不能删除）"
    );
}

// ---------- 5. 只暂停不删除 ----------

#[test]
fn pause_mode_never_terminates_target_on_cooldown() {
    let (_dir, state) = fixture_state("");
    assert_eq!(state.config.monitor.timeout_action, TimeoutAction::Pause);
    assert!(!state.config.monitor.pause_fallback_kill);
}

#[test]
fn rules_summary_exposes_strict_thresholds_to_frontend() {
    let (_dir, state) = fixture_state("");
    let s = commands::rules_summary(&state);
    assert_eq!(s.timeup_question_count, 3);
    assert_eq!(s.timeup_pass_score, 1.0);
}

#[test]
fn watch_config_target_defaults_to_enabled() {
    let text = r#"
        [[target_apps]]
        id = "bili"
        name = "哔哩哔哩"
        mac = "/Applications/bilibili.app"
        use_minutes = 10
    "#;
    let cfg = answergate_lib::config::parse_config(text).expect("parse");
    assert!(
        cfg.target_apps[0].watch,
        "独立进程监控默认开启（孩子自己点开也要被发现）"
    );
    assert_eq!(cfg.target_apps[0].use_minutes, 10.0);
}

#[test]
fn app_state_dir_is_under_temp_root() {
    // 冒烟：确保 fixture 目录独立，不污染真实状态文件
    let (dir, _state) = fixture_state("");
    let p: PathBuf = dir.path().to_path_buf();
    assert!(p.exists());
}

// ---------- 6. 重试必须沿用同一档门槛 ----------

#[test]
fn retry_keeps_strict_three_questions_for_timeup_round() {
    let (_dir, state) = fixture_state("");
    // 进入超时续期卷（3 题）
    {
        let mut gate = state.gate.lock().unwrap();
        gate.record_result(true, QuizReason::Initial, 120, 1000);
        gate.begin_using("bili", 111, 1100, false).unwrap();
        gate.begin_timeup_paused(Level::Primary, Some("bili".into()), 1200, PauseState::Paused)
            .unwrap();
    }
    // 答错 → Failed（还有重试余额）
    let paper_id = {
        let mut gate = state.gate.lock().unwrap();
        let p = answergate_lib::quiz::make_paper(
            &state.bank,
            Level::Primary,
            commands::paper_count_for(&state, QuizReason::Timeup),
            false,
        )
        .unwrap();
        let id = p.paper_id.clone();
        gate.set_active_paper(p);
        id
    };
    let wrong: Vec<answergate_lib::model::Answer> = {
        let gate = state.gate.lock().unwrap();
        gate.active_paper()
            .unwrap()
            .questions
            .iter()
            .map(|q| answergate_lib::model::Answer {
                question_id: q.id.clone(),
                value: " definitely_wrong ".into(),
            })
            .collect()
    };
    let r = commands::do_submit_answers(&state, &paper_id, &wrong, 1300).unwrap();
    assert!(!r.passed);
    assert_eq!(state.gate.lock().unwrap().phase, GatePhase::Failed);

    // 重试：题量必须仍是 3（不能借重试降回常规 5 题）
    let retry_paper = commands::do_retry_quiz(&state, 1400).expect("重试应开卷成功");
    assert_eq!(
        retry_paper.items.len(),
        3,
        "超时续期的重试卷必须仍是 3 题"
    );
}

#[test]
fn retry_keeps_strict_three_questions_for_watch_round() {
    let (_dir, state) = fixture_state("");
    {
        let mut gate = state.gate.lock().unwrap();
        gate.begin_watch_intercept(
            Level::Primary,
            Some("bili".into()),
            222,
            1000,
            PauseState::Paused,
        )
        .unwrap();
    }
    let paper_id = {
        let mut gate = state.gate.lock().unwrap();
        let p = answergate_lib::quiz::make_paper(
            &state.bank,
            Level::Primary,
            commands::paper_count_for(&state, QuizReason::Watch),
            false,
        )
        .unwrap();
        let id = p.paper_id.clone();
        gate.set_active_paper(p);
        id
    };
    let wrong: Vec<answergate_lib::model::Answer> = {
        let gate = state.gate.lock().unwrap();
        gate.active_paper()
            .unwrap()
            .questions
            .iter()
            .map(|q| answergate_lib::model::Answer {
                question_id: q.id.clone(),
                value: " definitely_wrong ".into(),
            })
            .collect()
    };
    let r = commands::do_submit_answers(&state, &paper_id, &wrong, 1100).unwrap();
    assert!(!r.passed);

    let retry_paper = commands::do_retry_quiz(&state, 1200).expect("重试应开卷成功");
    assert_eq!(
        retry_paper.items.len(),
        3,
        "监控拦截的重试卷必须仍是 3 题"
    );
}
