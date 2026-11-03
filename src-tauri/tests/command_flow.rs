//! Command 层核心逻辑串接测试（T04a）：
//! 判分 → 状态机 → 冷却推进 → 持久化恢复 → 家长密码，全部走 AppState + do_* 函数，
//! 不依赖 Tauri mock（AppHandle 相关仅在 command 薄封装中）。

use answergate_lib::commands::{self, AppState};
use answergate_lib::error::AppError;
use answergate_lib::gate::{GatePhase, GateState};
use answergate_lib::model::{Answer, Level, QuestionType};

const NOW: i64 = 2_000_000_000;

/// 建一个最小 fixture：app_data 与题库同在临时目录。
/// primary 6 题（3 单选 + 1 判断 + 2 填空），另两学段各 1 题占位。
fn fixture_state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let bank_dir = dir.path().join("bank");
    std::fs::create_dir_all(&bank_dir).unwrap();

    let primary: Vec<serde_json::Value> = (0..3)
        .map(|i| {
            serde_json::json!({
                "id": format!("p_{:03}", i), "level": "primary", "grade": null,
                "subject": "math", "type": "single_choice",
                "title": format!("第{}题", i), "options": ["1", "2", "3"],
                "answer": "2", "hint": null
            })
        })
        .chain([
            serde_json::json!({
                "id": "p_100", "level": "primary", "grade": null, "subject": "math",
                "type": "true_false", "title": "判断题", "options": [],
                "answer": "对", "hint": null
            }),
            serde_json::json!({
                "id": "p_101", "level": "primary", "grade": null, "subject": "math",
                "type": "fill_blank", "title": "填空1", "options": [],
                "answer": "42|四十二", "hint": null
            }),
            serde_json::json!({
                "id": "p_102", "level": "primary", "grade": null, "subject": "math",
                "type": "fill_blank", "title": "填空2", "options": [],
                "answer": "56", "hint": null
            }),
        ])
        .collect();
    std::fs::write(
        bank_dir.join("primary.json"),
        serde_json::to_string(&primary).unwrap(),
    )
    .unwrap();
    for (level, prefix) in [("kindergarten", "k"), ("junior", "j")] {
        let q = serde_json::json!({
            "id": format!("{}_filler", prefix), "level": level, "grade": null,
            "subject": "math", "type": "single_choice", "title": "占位",
            "options": ["A", "B"], "answer": "A", "hint": null
        });
        std::fs::write(
            bank_dir.join(format!("{}.json", level)),
            serde_json::to_string(&vec![q]).unwrap(),
        )
        .unwrap();
    }
    let state = AppState::new(dir.path().to_path_buf(), &bank_dir).unwrap();
    (dir, state)
}

/// 从当前活动试卷构造全错作答。
fn wrong_answers(state: &AppState) -> Vec<Answer> {
    let gate = state.gate.lock().unwrap();
    gate.active_paper()
        .unwrap()
        .questions
        .iter()
        .map(|q| Answer {
            question_id: q.id.clone(),
            value: " definitely_wrong ".into(),
        })
        .collect()
}

/// 从当前活动试卷构造全对作答（Rust 侧可见答案）。
fn right_answers(state: &AppState) -> Vec<Answer> {
    let gate = state.gate.lock().unwrap();
    gate.active_paper()
        .unwrap()
        .questions
        .iter()
        .map(|q| Answer {
            question_id: q.id.clone(),
            value: match q.qtype {
                QuestionType::FillBlank => q.answer.split('|').next().unwrap().to_string(),
                _ => q.answer.clone(),
            },
        })
        .collect()
}

#[test]
fn command_flow_fail_retry_cooldown_full_path() {
    let (_d, st) = fixture_state();
    // 规则摘要与目标列表（默认模板含 1 个演示目标）
    assert_eq!(commands::rules_summary(&st).question_count, 5);
    let targets = commands::list_target_views(&st);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].id, "demo-notes");

    // ① start_quiz：返回剥离答案的试卷视图
    let view = commands::do_start_quiz(&st, Level::Primary, None, NOW).unwrap();
    assert_eq!(view.items.len(), 5);
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("\"answer\""), "PaperView 泄漏 answer");
    assert!(!json.contains("\"hint\""), "PaperView 泄漏 hint");

    // ② 错误 paper_id → PaperNotFound
    assert_eq!(
        commands::do_submit_answers(&st, "bad_id", &[], NOW + 1)
            .unwrap_err()
            .code(),
        "PaperNotFound"
    );

    // ③ 三次机会全部失败：FAIL(rem2) → FAIL(rem1) → COOLDOWN(rem0)
    let g1 = commands::do_submit_answers(&st, &view.paper_id, &wrong_answers(&st), NOW + 1)
        .unwrap();
    assert!(!g1.passed);
    assert_eq!(g1.remaining_retry, 2);

    let v2 = commands::do_retry_quiz(&st, NOW + 2).unwrap();
    let g2 = commands::do_submit_answers(&st, &v2.paper_id, &wrong_answers(&st), NOW + 3).unwrap();
    assert_eq!(g2.remaining_retry, 1);

    let v3 = commands::do_retry_quiz(&st, NOW + 4).unwrap();
    let g3 = commands::do_submit_answers(&st, &v3.paper_id, &wrong_answers(&st), NOW + 5).unwrap();
    assert_eq!(g3.remaining_retry, 0);

    // ④ 冷却中：gate_view 显示 COOLDOWN 与剩余秒数；start_quiz/retry 均被拒
    let gv = commands::gate_view(&st, NOW + 6);
    assert_eq!(gv.phase, GatePhase::Cooldown);
    assert!(gv.cooldown_remaining_sec > 0);
    match commands::do_start_quiz(&st, Level::Primary, None, NOW + 7) {
        Err(AppError::CooldownActive { remaining_sec }) => assert!(remaining_sec > 0),
        other => panic!("期望 CooldownActive，实际 {:?}", other),
    }
    assert_eq!(
        commands::do_retry_quiz(&st, NOW + 8).unwrap_err().code(),
        "CooldownActive"
    );

    // ⑤ 持久化恢复：从磁盘 recover 冷却不重置（删 config.toml 亦不影响——独立文件）
    std::fs::remove_file(st.app_data_dir.join("config.toml")).unwrap();
    let recovered = GateState::recover(&st.state_file(), 2, NOW + 9);
    assert_eq!(recovered.phase, GatePhase::Cooldown);
    // 冷却到点 → 复位且余额满额
    let done = GateState::recover(&st.state_file(), 2, NOW + 6 + 200);
    assert_eq!(done.phase, GatePhase::Idle);
    assert_eq!(done.remaining_retry, 2);
}

#[test]
fn command_flow_pass_path_reaches_passed() {
    let (_d, st) = fixture_state();
    let view = commands::do_start_quiz(&st, Level::Primary, None, NOW).unwrap();
    // 用 Rust 侧可见答案构造全对作答（4/5=0.8 恰及格也算通过，这里全对 5/5）
    let result =
        commands::do_submit_answers(&st, &view.paper_id, &right_answers(&st), NOW + 1).unwrap();
    assert!(result.passed, "全对应通过");
    assert_eq!(commands::gate_view(&st, NOW + 2).phase, GatePhase::Passed);
}

#[test]
fn command_flow_partial_correct_at_pass_line() {
    let (_d, st) = fixture_state();
    // 4/5 = 0.8 恰好及格：错第一题、其余全对
    let view = commands::do_start_quiz(&st, Level::Primary, None, NOW).unwrap();
    let mut answers: Vec<Answer> = {
        let gate = st.gate.lock().unwrap();
        gate.active_paper()
            .unwrap()
            .questions
            .iter()
            .enumerate()
            .map(|(i, q)| Answer {
                question_id: q.id.clone(),
                value: if i == 0 {
                    "wrong".into()
                } else {
                    match q.qtype {
                        QuestionType::FillBlank => {
                            q.answer.split('|').next().unwrap().to_string()
                        }
                        _ => q.answer.clone(),
                    }
                },
            })
            .collect()
    };
    let _ = &view;
    let result = commands::do_submit_answers(&st, &view.paper_id, &answers, NOW + 1).unwrap();
    std::mem::take(&mut answers);
    assert!(result.passed, "4/5=0.8 恰好达到及格线应通过");
}

#[test]
fn command_flow_start_quiz_target_not_found() {
    let (_d, st) = fixture_state();
    let err =
        commands::do_start_quiz(&st, Level::Primary, Some("no_such_target".into()), NOW).unwrap_err();
    assert_eq!(err.code(), "TargetNotFound");
}

#[test]
fn parent_unlock_rejected_when_disabled_or_wrong() {
    let (_d, st) = fixture_state();
    // 默认模板未启用家长密码 → 拒绝
    let err = commands::verify_parent_password(&st, "any").unwrap_err();
    assert_eq!(err.code(), "ParentPasswordError");
}

#[test]
fn parent_unlock_verifies_sha256_hash() {
    let dir = tempfile::tempdir().unwrap();
    let bank_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../resources/question_bank");
    // 手写启用家长密码的 config（sha256("answer-gate") 的 hex）
    let hash = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"answer-gate");
        h.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>()
    };
    std::fs::write(
        dir.path().join("config.toml"),
        format!("[parent]\npassword_hash = \"{}\"\n", hash),
    )
    .unwrap();
    let st = AppState::new(dir.path().to_path_buf(), &bank_dir).unwrap();
    // 错误密码 → ParentPasswordError；正确密码 → Ok
    assert_eq!(
        commands::verify_parent_password(&st, "wrong").unwrap_err().code(),
        "ParentPasswordError"
    );
    assert!(commands::verify_parent_password(&st, "answer-gate").is_ok());
}

#[test]
fn seed_banks_load_through_app_state() {
    // 真实种子题库（284 题）通过 AppState 全链路加载
    let dir = tempfile::tempdir().unwrap();
    let bank_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../resources/question_bank");
    let st = AppState::new(dir.path().to_path_buf(), &bank_dir).unwrap();
    assert_eq!(st.bank.len(), 284, "种子题库应为 68+106+110=284 题");
    for level in Level::ALL {
        // 确定性断言 1：每学段题库三种题型均存在（不依赖随机抽题结果）
        let pool = answergate_lib::quiz::filter_pool(&st.bank, level, None, None);
        assert!(pool.len() >= 50, "学段 {:?} 题量不足", level);
        for t in [
            QuestionType::SingleChoice,
            QuestionType::TrueFalse,
            QuestionType::FillBlank,
        ] {
            assert!(
                pool.iter().any(|q| q.qtype == t),
                "学段 {:?} 缺少题型 {:?}",
                level,
                t
            );
        }
        // 确定性断言 2：抽 5 题数量正确且 id 互不重复（去重，与抽中哪些题无关）
        let paper = answergate_lib::quiz::make_paper(&st.bank, level, 5, false).unwrap();
        assert_eq!(paper.questions.len(), 5);
        let mut ids: Vec<&str> = paper.questions.iter().map(|q| q.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5, "学段 {:?} 抽题出现重复 id", level);
    }
}
