//! 核心内核集成测试（T02 完成标准全覆盖）：
//! 1. 非法 JSON 友好报错（AppError code）
//! 2. 抽题去重、题量不足回退、Android 过滤填空
//! 3. 判分边界：全对/全错/恰及格、填空全角/大小写/多答案
//! 4. 状态机全路径：PASS、FAIL→RETRY 递减、耗尽→COOLDOWN、冷却中拒绝 start_quiz
//! 5. 持久化恢复：save→load 一致；状态文件与 config.toml 相互独立
//! 6. 题库校验：重复 id、答案不在选项内、空题库
//! 7. 配置默认值回退与非法值清洗

use answergate_lib::config;
use answergate_lib::error::AppError;
use answergate_lib::gate::{GatePhase, GateState};
use answergate_lib::model::{Answer, Level, Paper, Question, QuestionType};
use answergate_lib::quiz;

// ---------- 测试工具 ----------

/// 构造一道题的 JSON（严格符合 schema）。
fn question_json(
    id: &str,
    level: Level,
    qtype: QuestionType,
    title: &str,
    options: &[&str],
    answer: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "level": level,
        "grade": null,
        "subject": "math",
        "type": qtype,
        "title": title,
        "options": options,
        "answer": answer,
        "hint": null
    })
}

/// 写题库文件到临时目录。
fn write_bank(dir: &Path, file: &str, content: &str) {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

use std::path::Path;

/// 建一个临时题库目录：指定学段 count 道 qtype 题，其余学段各 1 道单选占位题。
fn bank_with(level: Level, count: usize, qtype: QuestionType) -> (tempfile::TempDir, Vec<Question>) {
    let dir = tempfile::tempdir().unwrap();
    let prefix = level.bank_file().trim_end_matches(".json");
    let mut questions = Vec::new();
    for i in 0..count {
        let (options, answer): (Vec<&str>, &str) = match qtype {
            QuestionType::SingleChoice => (vec!["1", "2", "3", "4"], "2"),
            QuestionType::TrueFalse => (vec![], "对"),
            QuestionType::FillBlank => (vec![], "42"),
        };
        questions.push(question_json(
            &format!("{}_{}", prefix, i),
            level,
            qtype,
            &format!("第{}题", i),
            &options,
            answer,
        ));
    }
    write_bank(
        dir.path(),
        level.bank_file(),
        &serde_json::to_string(&questions).unwrap(),
    );
    for other in Level::ALL {
        if other != level {
            let arr = vec![question_json(
                &format!("{}_filler", other.bank_file().trim_end_matches(".json")),
                other,
                QuestionType::SingleChoice,
                "占位题",
                &["A", "B"],
                "A",
            )];
            write_bank(
                dir.path(),
                other.bank_file(),
                &serde_json::to_string(&arr).unwrap(),
            );
        }
    }
    let bank = quiz::load_bank(dir.path()).unwrap();
    (dir, bank)
}

/// 构造内存单选题池。
fn single_choice_pool(level: Level, count: usize) -> Vec<Question> {
    (0..count)
        .map(|i| Question {
            id: format!("{}_q{:03}", level.bank_file().trim_end_matches(".json"), i),
            level,
            grade: None,
            subject: "math".to_string(),
            qtype: QuestionType::SingleChoice,
            title: format!("第{}题", i),
            options: vec!["1".into(), "2".into(), "3".into()],
            answer: "2".into(),
            hint: None,
        })
        .collect()
}

// ---------- 1. 非法 JSON 友好报错 ----------

#[test]
fn invalid_json_returns_friendly_code() {
    let dir = tempfile::tempdir().unwrap();
    write_bank(dir.path(), "kindergarten.json", "{ 这不是合法 JSON !!!");
    write_bank(
        dir.path(),
        "primary.json",
        r#"[{"id":"p1","level":"primary","subject":"math","type":"single_choice","title":"t","options":["1"],"answer":"1"}]"#,
    );
    write_bank(
        dir.path(),
        "junior.json",
        r#"[{"id":"j1","level":"junior","subject":"math","type":"single_choice","title":"t","options":["1"],"answer":"1"}]"#,
    );
    let err = quiz::load_bank(dir.path()).unwrap_err();
    assert_eq!(err.code(), "QuestionBank");
    assert!(err.to_string().contains("解析失败"));
}

#[test]
fn missing_bank_file_returns_friendly_code() {
    let dir = tempfile::tempdir().unwrap();
    let err = quiz::load_bank(dir.path()).unwrap_err();
    assert_eq!(err.code(), "QuestionBank");
    assert!(err.to_string().contains("读取失败"));
}

// ---------- 2. 抽题：去重 / 题量不足 / 过滤填空 ----------

#[test]
fn pick_questions_are_deduplicated() {
    let pool = single_choice_pool(Level::Primary, 20);
    for _ in 0..20 {
        let (picked, degraded) = quiz::pick_questions(pool.clone(), 5, false).unwrap();
        assert!(!degraded);
        let mut ids: Vec<&str> = picked.iter().map(|q| q.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5, "抽题结果不允许重复 id");
    }
}

#[test]
fn pick_all_when_pool_smaller_than_requested() {
    let pool = single_choice_pool(Level::Primary, 3);
    let (picked, degraded) = quiz::pick_questions(pool, 5, false).unwrap();
    assert_eq!(picked.len(), 3, "题量不足时回退为取全部");
    assert!(degraded, "降级标记必须为 true");
}

#[test]
fn empty_pool_after_filter_returns_not_enough_questions() {
    // 全填空题池 + Android 过滤 → 无题可抽 → 明确报错
    let pool: Vec<Question> = (0..3)
        .map(|i| Question {
            id: format!("fb_{}", i),
            level: Level::Primary,
            grade: None,
            subject: "math".to_string(),
            qtype: QuestionType::FillBlank,
            title: format!("填空{}", i),
            options: vec![],
            answer: "7".into(),
            hint: None,
        })
        .collect();
    let err = quiz::pick_questions(pool, 5, true).unwrap_err();
    match err {
        AppError::NotEnoughQuestions { requested, available } => {
            assert_eq!(requested, 5);
            assert_eq!(available, 0);
        }
        other => panic!("期望 NotEnoughQuestions，实际：{:?}", other),
    }
}

#[test]
fn exclude_fill_blank_on_pure_fill_blank_bank_errors() {
    // 题库指定学段全是填空题，Android 过滤后无题可抽 → 明确报错
    let (_dir, bank) = bank_with(Level::Primary, 10, QuestionType::FillBlank);
    let pool = quiz::filter_pool(&bank, Level::Primary, None, None);
    assert_eq!(pool.len(), 10);
    let err = quiz::pick_questions(pool, 5, true).unwrap_err();
    assert_eq!(err.code(), "NotEnoughQuestions");
}

#[test]
fn exclude_fill_blank_keeps_choice_questions() {
    // 混合题库：5 单选 + 5 填空，过滤后只可能出单选
    let mut mixed = single_choice_pool(Level::Primary, 5);
    for i in 0..5 {
        mixed.push(Question {
            id: format!("fb_{}", i),
            level: Level::Primary,
            grade: None,
            subject: "math".to_string(),
            qtype: QuestionType::FillBlank,
            title: format!("填空{}", i),
            options: vec![],
            answer: "7".into(),
            hint: None,
        });
    }
    let (picked, _) = quiz::pick_questions(mixed, 5, true).unwrap();
    assert!(picked.iter().all(|q| q.qtype == QuestionType::SingleChoice));
}

#[test]
fn make_paper_strips_nothing_but_view_strips_answers() {
    let (_dir, bank) = bank_with(Level::Primary, 10, QuestionType::SingleChoice);
    let paper = quiz::make_paper(&bank, Level::Primary, 5, false).unwrap();
    assert_eq!(paper.questions.len(), 5);
    let view = paper.to_view();
    assert_eq!(view.items.len(), 5);
    // PaperView / QuizItem 不含 answer/hint 字段（类型层面保证，序列化输出再验一次）
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("answer"), "PaperView 泄漏 answer 字段");
    assert!(!json.contains("hint"), "PaperView 泄漏 hint 字段");
}

// ---------- 3. 判分边界 ----------

/// 五题整卷：2 单选 + 1 判断 + 2 填空。
fn fill_paper() -> Paper {
    Paper {
        paper_id: "p_test".into(),
        level: Level::Primary,
        questions: vec![
            Question { id: "q1".into(), level: Level::Primary, grade: None, subject: "math".into(), qtype: QuestionType::SingleChoice, title: "1".into(), options: vec!["8".into(), "9".into()], answer: "8".into(), hint: None },
            Question { id: "q2".into(), level: Level::Primary, grade: None, subject: "math".into(), qtype: QuestionType::SingleChoice, title: "2".into(), options: vec!["10".into(), "11".into()], answer: "10".into(), hint: None },
            Question { id: "q3".into(), level: Level::Primary, grade: None, subject: "math".into(), qtype: QuestionType::TrueFalse, title: "3".into(), options: vec![], answer: "对".into(), hint: None },
            Question { id: "q4".into(), level: Level::Primary, grade: None, subject: "math".into(), qtype: QuestionType::FillBlank, title: "4".into(), options: vec![], answer: "42".into(), hint: None },
            Question { id: "q5".into(), level: Level::Primary, grade: None, subject: "math".into(), qtype: QuestionType::FillBlank, title: "5".into(), options: vec![], answer: "北京|Beijing".into(), hint: None },
        ],
    }
}

#[test]
fn grade_all_correct() {
    let paper = fill_paper();
    let answers = vec![
        Answer { question_id: "q1".into(), value: "8".into() },
        Answer { question_id: "q2".into(), value: "10".into() },
        Answer { question_id: "q3".into(), value: "对".into() },
        Answer { question_id: "q4".into(), value: "42".into() },
        Answer { question_id: "q5".into(), value: "北京".into() },
    ];
    let r = quiz::grade(&paper, &answers, 0.8);
    assert_eq!(r.correct, 5);
    assert_eq!(r.total, 5);
    assert!(r.passed);
}

#[test]
fn grade_all_wrong_including_unanswered() {
    let paper = fill_paper();
    let r = quiz::grade(&paper, &[], 0.8);
    assert_eq!(r.correct, 0);
    assert!(!r.passed);
    assert_eq!(r.details.len(), 5, "未作答也应有对错明细");
    assert!(r.details.iter().all(|d| !d.correct));
}

#[test]
fn grade_boundary_exactly_at_pass_line() {
    let paper = fill_paper();
    // 4/5 = 0.8，恰好等于及格线 → 通过
    let answers = vec![
        Answer { question_id: "q1".into(), value: "8".into() },
        Answer { question_id: "q2".into(), value: "10".into() },
        Answer { question_id: "q3".into(), value: "对".into() },
        Answer { question_id: "q4".into(), value: "42".into() },
        Answer { question_id: "q5".into(), value: "答错".into() },
    ];
    let r = quiz::grade(&paper, &answers, 0.8);
    assert_eq!(r.correct, 4);
    assert!(r.passed, "正确率恰好等于及格线应判通过");
    // 3/5 = 0.6 < 0.8 → 不通过
    let r3 = quiz::grade(&paper, &answers[..3], 0.8);
    assert!(!r3.passed);
}

#[test]
fn fill_blank_fullwidth_case_and_multi_answer() {
    let base = Question {
        id: "fb".into(),
        level: Level::Primary,
        grade: None,
        subject: "math".into(),
        qtype: QuestionType::FillBlank,
        title: "t".into(),
        options: vec![],
        answer: "42".into(),
        hint: None,
    };
    // 全角数字 → 半角；带空格
    assert!(quiz::is_correct(&base, "４２"));
    assert!(quiz::is_correct(&base, " 42 "));
    assert!(!quiz::is_correct(&base, "43"));
    // 忽略大小写 + 多答案 | 分隔
    let multi = Question { answer: "Beijing|北京".into(), ..base.clone() };
    assert!(quiz::is_correct(&multi, "beijing"));
    assert!(quiz::is_correct(&multi, "BEIJING"));
    assert!(quiz::is_correct(&multi, "北京"));
    assert!(!quiz::is_correct(&multi, "上海"));
    // 单选答案同样归一化（全角/大小写）
    let choice = Question {
        qtype: QuestionType::SingleChoice,
        options: vec!["Ａ".into(), "Ｂ".into()],
        answer: "Ａ".into(),
        ..base
    };
    assert!(quiz::is_correct(&choice, "a"));
}

// ---------- 4. 状态机全路径 ----------

const NOW: i64 = 1_000_000;
const COOLDOWN: u64 = 120;

#[test]
fn state_machine_pass_path() {
    let mut st = GateState::new(2);
    st.begin_quiz(Level::Primary, None, NOW).unwrap();
    assert_eq!(st.phase, GatePhase::Quiz);
    assert_eq!(st.record_result(true, COOLDOWN, NOW), GatePhase::Passed);
    assert_eq!(st.remaining_retry, 2, "通过不扣减重试次数");
}

#[test]
fn state_machine_fail_retry_then_cooldown() {
    // max_retry=2 → 首答 + 2 次重试共 3 次机会；重试余额在 begin_retry 时扣减
    let mut st = GateState::new(2);
    // 第 1 次失败：FAIL，剩余重试仍显示满额 2
    st.begin_quiz(Level::Primary, None, NOW).unwrap();
    assert_eq!(st.record_result(false, COOLDOWN, NOW), GatePhase::Failed);
    assert_eq!(st.remaining_retry, 2);
    // 第 1 次重试（余额 2→1）→ QUIZ → 第 2 次失败：FAIL
    st.begin_retry(NOW).unwrap();
    assert_eq!(st.phase, GatePhase::Quiz);
    assert_eq!(st.record_result(false, COOLDOWN, NOW), GatePhase::Failed);
    assert_eq!(st.remaining_retry, 1);
    // 第 2 次重试（余额 1→0）→ QUIZ → 第 3 次失败：余额为 0 → COOLDOWN
    st.begin_retry(NOW).unwrap();
    assert_eq!(st.record_result(false, COOLDOWN, NOW), GatePhase::Cooldown);
    assert_eq!(st.remaining_retry, 0);
    assert_eq!(st.cooldown_until_epoch, Some(NOW + COOLDOWN as i64));
    // 冷却期间再试 → 优先命中 CooldownActive（冷却校验在前）
    assert!(matches!(
        st.begin_retry(NOW),
        Err(AppError::CooldownActive { .. })
    ));
}

#[test]
fn cooldown_blocks_start_quiz_with_remaining_seconds() {
    let mut st = GateState::new(2);
    // 三次机会全部用完 → COOLDOWN
    st.begin_quiz(Level::Primary, None, NOW).unwrap();
    st.record_result(false, COOLDOWN, NOW);
    st.begin_retry(NOW).unwrap();
    st.record_result(false, COOLDOWN, NOW);
    st.begin_retry(NOW).unwrap();
    st.record_result(false, COOLDOWN, NOW); // rem=0 → 冷却
    assert_eq!(st.phase, GatePhase::Cooldown);
    // 冷却未到，start_quiz 被拒绝并携带剩余秒数
    let err = st.begin_quiz(Level::Primary, None, NOW + 10).unwrap_err();
    match err {
        AppError::CooldownActive { remaining_sec } => {
            assert_eq!(remaining_sec, COOLDOWN - 10);
        }
        other => panic!("期望 CooldownActive，实际：{:?}", other),
    }
    // 冷却结束后可正常开始，重试余额恢复满额
    st.finish_cooldown_if_elapsed(NOW + COOLDOWN as i64 + 1);
    assert_eq!(st.phase, GatePhase::Idle);
    st.begin_quiz(Level::Primary, None, NOW + COOLDOWN as i64 + 2).unwrap();
    assert_eq!(st.remaining_retry, 2);
}

// ---------- 5. 持久化恢复 ----------

#[test]
fn gate_state_persist_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".answergate_gate_state.json");
    let mut st = GateState::new(2);
    st.begin_quiz(Level::Junior, Some("target-1".into()), NOW).unwrap();
    st.record_result(false, COOLDOWN, NOW); // Failed, 显示余额 2
    st.begin_retry(NOW).unwrap(); // 余额 2→1，回 QUIZ
    st.save(&path).unwrap();
    let loaded = GateState::load(&path).unwrap().unwrap();
    assert_eq!(loaded.phase, GatePhase::Quiz);
    assert_eq!(loaded.remaining_retry, 1);
    assert_eq!(loaded.current_level, Some(Level::Junior));
    assert_eq!(loaded.selected_target_id, Some("target-1".into()));
}

#[test]
fn recover_preserves_cooldown_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    // 冷却中保存（直接构造 rem=0 的失败态再判分，即冷却触发条件）
    let mut st = GateState::new(2);
    st.remaining_retry = 0;
    st.record_result(false, COOLDOWN, NOW);
    assert_eq!(st.phase, GatePhase::Cooldown);
    st.save(&path).unwrap();
    // 重启：剩余时间未到 → 仍处冷却
    let recovered = GateState::recover(&path, 2, NOW + 20);
    assert_eq!(recovered.phase, GatePhase::Cooldown);
    assert_eq!(recovered.cooldown_remaining(NOW + 20), Some(COOLDOWN - 20));
    // 重启：冷却已过 → Idle 且余额满额
    let recovered2 = GateState::recover(&path, 2, NOW + COOLDOWN as i64 + 5);
    assert_eq!(recovered2.phase, GatePhase::Idle);
    assert_eq!(recovered2.remaining_retry, 2);
}

#[test]
fn gate_state_independent_of_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join(".answergate_gate_state.json");
    let config_path = dir.path().join("config.toml");
    // 持久化一个冷却中状态（重试余额为 0 时失败 → 触发冷却）
    let mut st = GateState::new(2);
    st.remaining_retry = 0;
    st.record_result(false, COOLDOWN, NOW);
    st.save(&state_path).unwrap();
    // 删除 config.toml
    std::fs::write(&config_path, "[quiz]\nquestion_count = 9\n").unwrap();
    std::fs::remove_file(&config_path).unwrap();
    // config 缺失回退默认值，门禁状态不受影响
    let cfg = config::load_or_default(&config_path);
    assert_eq!(cfg.quiz, QuizConfig::default());
    let recovered = GateState::recover(&state_path, cfg.quiz.max_retry, NOW + 1);
    assert_eq!(recovered.phase, GatePhase::Cooldown);
}

use answergate_lib::model::QuizConfig;

#[test]
fn corrupted_state_file_resets_to_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    std::fs::write(&path, "垃圾数据{{{").unwrap();
    let st = GateState::recover(&path, 2, NOW);
    assert_eq!(st.phase, GatePhase::Idle);
    assert_eq!(st.remaining_retry, 2);
}

// ---------- 6. 题库校验 ----------

#[test]
fn bank_rejects_duplicate_ids() {
    let dir = tempfile::tempdir().unwrap();
    let dup = vec![
        question_json("dup_1", Level::Primary, QuestionType::SingleChoice, "甲", &["A", "B"], "A"),
        question_json("dup_1", Level::Primary, QuestionType::SingleChoice, "乙", &["A", "B"], "B"),
    ];
    write_bank(dir.path(), "primary.json", &serde_json::to_string(&dup).unwrap());
    let arr = vec![question_json("j_1", Level::Junior, QuestionType::SingleChoice, "t", &["A"], "A")];
    write_bank(dir.path(), "junior.json", &serde_json::to_string(&arr).unwrap());
    let arr = vec![question_json("k_1", Level::Kindergarten, QuestionType::SingleChoice, "t", &["A"], "A")];
    write_bank(dir.path(), "kindergarten.json", &serde_json::to_string(&arr).unwrap());
    let err = quiz::load_bank(dir.path()).unwrap_err();
    assert_eq!(err.code(), "QuestionBank");
    assert!(err.to_string().contains("id 重复"));
}

#[test]
fn bank_rejects_answer_not_in_options() {
    let dir = tempfile::tempdir().unwrap();
    let bad = vec![question_json(
        "sc_1",
        Level::Primary,
        QuestionType::SingleChoice,
        "3+5等于几？",
        &["6", "8"],
        "9", // 不在选项内
    )];
    write_bank(dir.path(), "primary.json", &serde_json::to_string(&bad).unwrap());
    let arr = vec![question_json("j_1", Level::Junior, QuestionType::SingleChoice, "t", &["A"], "A")];
    write_bank(dir.path(), "junior.json", &serde_json::to_string(&arr).unwrap());
    let arr = vec![question_json("k_1", Level::Kindergarten, QuestionType::SingleChoice, "t", &["A"], "A")];
    write_bank(dir.path(), "kindergarten.json", &serde_json::to_string(&arr).unwrap());
    let err = quiz::load_bank(dir.path()).unwrap_err();
    assert!(err.to_string().contains("不在选项中"));
}

#[test]
fn bank_rejects_invalid_truefalse_answer() {
    let err = quiz::validate_questions(
        &[Question {
            id: "tf_1".into(),
            level: Level::Primary,
            grade: None,
            subject: "science".into(),
            qtype: QuestionType::TrueFalse,
            title: "t".into(),
            options: vec![],
            answer: "正确".into(), // 非法，应为「对/错」
            hint: None,
        }],
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("非法"));
}

#[test]
fn bank_rejects_level_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    // kindergarten.json 里混入 primary 题
    let mixed = vec![question_json(
        "wrong_level",
        Level::Primary,
        QuestionType::SingleChoice,
        "t",
        &["A"],
        "A",
    )];
    write_bank(dir.path(), "kindergarten.json", &serde_json::to_string(&mixed).unwrap());
    let arr = vec![question_json("j_1", Level::Junior, QuestionType::SingleChoice, "t", &["A"], "A")];
    write_bank(dir.path(), "junior.json", &serde_json::to_string(&arr).unwrap());
    let arr = vec![question_json("p_1", Level::Primary, QuestionType::SingleChoice, "t", &["A"], "A")];
    write_bank(dir.path(), "primary.json", &serde_json::to_string(&arr).unwrap());
    let err = quiz::load_bank(dir.path()).unwrap_err();
    assert!(err.to_string().contains("与所属文件不符"));
}

// ---------- 7. 配置默认值回退 ----------

#[test]
fn config_missing_fields_fall_back_to_defaults() {
    let cfg = config::parse_config("[quiz]\nquestion_count = 10\n").unwrap();
    assert_eq!(cfg.quiz.question_count, 10);
    assert_eq!(cfg.quiz.pass_score, 0.8, "缺失字段回退默认值");
    assert_eq!(cfg.quiz.max_retry, 2);
    assert_eq!(cfg.quiz.cool_down_sec, 120);
    assert_eq!(cfg.quiz.default_level, Level::Primary);
}

#[test]
fn config_invalid_values_are_sanitized() {
    let cfg = config::parse_config(
        "[quiz]\nquestion_count = 0\npass_score = 1.5\ncool_down_sec = 0\n",
    )
    .unwrap();
    assert_eq!(cfg.quiz.question_count, 5);
    assert_eq!(cfg.quiz.pass_score, 0.8);
    assert_eq!(cfg.quiz.cool_down_sec, 120);
}

#[test]
fn config_malformed_toml_returns_config_error() {
    let err = config::parse_config("[quiz\nbroken===").unwrap_err();
    assert_eq!(err.code(), "Config");
}

#[test]
fn config_targets_dedup_by_id() {
    let cfg = config::parse_config(
        r#"
        [[target_apps]]
        id = "a"
        name = "AppA"
        mac = "/Applications/A.app"
        [[target_apps]]
        id = "a"
        name = "AppA-dup"
        "#,
    )
    .unwrap();
    assert_eq!(cfg.target_apps.len(), 1);
    assert_eq!(cfg.target_apps[0].name, "AppA");
}
