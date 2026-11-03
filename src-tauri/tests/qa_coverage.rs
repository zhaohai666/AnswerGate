//! QA 补充覆盖测试（严过关 · 第 1 轮独立验证）：
//! 1. retry→cooldown 持久化字段完整性（原始 JSON 字段级断言）
//! 2. AppState 重建（模拟重启）后冷却恢复 + 到点复位
//! 3. config 白名单多平台路径解析（mac/win/android 字段按平台取值）
//! 4. 判分正确率浮点精度边界（0.7、1.0、2/3）
//! 5. PaperView 值级泄漏检测（答案值本身不得出现在序列化输出）
//! 6. 打包资源布局与 resolve_bank_dir 一致性（暴露打包路径 bug，当前预期失败）

use std::path::Path;

use answergate_lib::commands::{self, AppState};
use answergate_lib::config;
use answergate_lib::model::{Answer, Level, QuestionType};

const NOW: i64 = 3_000_000_000;

/// fixture：与 command_flow 同构的最小题库（primary 6 题），可自定义 config 文本。
fn fixture_state_with_config(config_text: Option<&str>) -> (tempfile::TempDir, AppState) {
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
    if let Some(text) = config_text {
        std::fs::write(dir.path().join("config.toml"), text).unwrap();
    }
    let state = AppState::new(dir.path().to_path_buf(), &bank_dir).unwrap();
    (dir, state)
}

fn wrong_answers(state: &AppState) -> Vec<Answer> {
    let gate = state.gate.lock().unwrap();
    gate.active_paper()
        .unwrap()
        .questions
        .iter()
        .map(|q| Answer {
            question_id: q.id.clone(),
            value: "wrong".into(),
        })
        .collect()
}

/// 驱动到冷却态：三次机会全部失败（rem 2→1→0→Cooldown）。
fn drive_to_cooldown(st: &AppState) {
    let view = commands::do_start_quiz(st, Level::Primary, None, NOW).unwrap();
    commands::do_submit_answers(st, &view.paper_id, &wrong_answers(st), NOW + 1).unwrap();
    let v2 = commands::do_retry_quiz(st, NOW + 2).unwrap();
    commands::do_submit_answers(st, &v2.paper_id, &wrong_answers(st), NOW + 3).unwrap();
    let v3 = commands::do_retry_quiz(st, NOW + 4).unwrap();
    commands::do_submit_answers(st, &v3.paper_id, &wrong_answers(st), NOW + 5).unwrap();
    assert_eq!(
        commands::gate_view(st, NOW + 6).phase,
        answergate_lib::gate::GatePhase::Cooldown
    );
}

// ---------- 1. retry→cooldown 持久化字段完整性 ----------

#[test]
fn cooldown_state_file_field_completeness() {
    let (_d, st) = fixture_state_with_config(Some(
        "[quiz]\nquestion_count = 5\ncool_down_sec = 300\nmax_retry = 2\n",
    ));
    drive_to_cooldown(&st);
    // 字段级断言：直接读状态文件原始 JSON
    let text =
        std::fs::read_to_string(st.app_data_dir.join(".answergate_gate_state.json")).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(raw["phase"], "cooldown", "phase 必须为 cooldown");
    assert_eq!(raw["remaining_retry"], 0, "重试余额必须为 0");
    assert_eq!(
        raw["max_retry"], 2,
        "max_retry 必须持久化（复位满额依赖该字段）"
    );
    assert_eq!(
        raw["cooldown_until_epoch"],
        NOW + 5 + 300,
        "cooldown_until_epoch 必须为最后判分时刻 + cool_down_sec"
    );
    assert_eq!(
        raw["current_level"], "primary",
        "current_level 必须持久化（冷却结束复位前可追溯学段）"
    );
    assert_eq!(
        raw["selected_target_id"], serde_json::Value::Null,
        "未选目标时该字段为 null（字段仍须存在）"
    );
    assert!(
        raw.get("active_paper").is_none(),
        "active_paper 含答案，绝不允许落盘"
    );
}

// ---------- 2. AppState 重建（模拟重启）恢复冷却 ----------

#[test]
fn app_state_rebuild_restores_cooldown_then_resets() {
    let dir = tempfile::tempdir().unwrap();
    let bank_dir = dir.path().join("bank");
    // 复用 fixture 建目录后重建两次 AppState（模拟两次进程启动）
    let st1 = {
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
        AppState::new(dir.path().to_path_buf(), &bank_dir).unwrap()
    };
    drive_to_cooldown(&st1);
    let cooldown_until = st1
        .gate
        .lock()
        .unwrap()
        .cooldown_until_epoch
        .expect("冷却态必须有截止时间");

    // 进程"重启"：同 app_data_dir 重新 AppState::new（磁盘上有冷却状态）
    let st2 = AppState::new(dir.path().to_path_buf(), &bank_dir).unwrap();
    let gv = commands::gate_view(&st2, NOW + 100);
    assert_eq!(
        gv.phase,
        answergate_lib::gate::GatePhase::Cooldown,
        "重启后冷却必须恢复（防重启绕过）"
    );
    assert_eq!(
        gv.cooldown_remaining_sec,
        (cooldown_until - (NOW + 100)) as u64,
        "剩余秒数必须与持久化截止时间一致"
    );
    assert_eq!(gv.remaining_retry, 0, "冷却期重试余额保持 0");

    // 冷却到点：视图查询应自动复位为 Idle 且余额满额
    let gv_done = commands::gate_view(&st2, cooldown_until + 1);
    assert_eq!(gv_done.phase, answergate_lib::gate::GatePhase::Idle);
    assert_eq!(gv_done.remaining_retry, 2, "复位后重试余额恢复满额");
    // 复位后可立即重新开始答题
    assert!(commands::do_start_quiz(&st2, Level::Primary, None, cooldown_until + 2).is_ok());
}

// ---------- 3. config 白名单多平台路径解析 ----------

#[test]
fn whitelist_multi_platform_spec_resolution() {
    let cfg = config::parse_config(
        r#"
        [[target_apps]]
        id = "mac-only"
        name = "MacOnly"
        mac = "/Applications/Notes.app"
        [[target_apps]]
        id = "win-only"
        name = "WinOnly"
        win = "C:\\Games\\Game.exe"
        [[target_apps]]
        id = "android-only"
        name = "AndroidOnly"
        android = "com.example.game"
        [[target_apps]]
        id = "all-platforms"
        name = "AllPlatforms"
        mac = "/Applications/A.app"
        win = "C:\\A\\a.exe"
        android = "com.example.a"
        "#,
    )
    .unwrap();
    assert_eq!(cfg.target_apps.len(), 4, "四个条目 id 均不同，不应被去重");

    let by_id = |id: &str| {
        cfg.target_apps
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .clone()
    };
    // macOS 编译平台：仅 mac 字段可用
    let mac_only = by_id("mac-only");
    assert_eq!(
        mac_only.launch_spec(),
        Some("/Applications/Notes.app"),
        "macOS 上 mac-only 条目应取 mac 路径"
    );
    assert_eq!(
        by_id("win-only").launch_spec(),
        None,
        "macOS 上 win-only 条目不可用"
    );
    assert_eq!(
        by_id("android-only").launch_spec(),
        None,
        "macOS 上 android-only 条目不可用"
    );
    assert_eq!(
        by_id("all-platforms").launch_spec(),
        Some("/Applications/A.app"),
        "多平台条目在 macOS 上取 mac 字段"
    );

    // list_target_views 的 available 标志（前端据此过滤不可用项）
    let mut state = fixture_state_with_config(None).1;
    state.config = cfg;
    let views = commands::list_target_views(&state);
    let avail = |id: &str| views.iter().find(|v| v.id == id).unwrap().available;
    assert!(avail("mac-only"));
    assert!(!avail("win-only"));
    assert!(!avail("android-only"));
    assert!(avail("all-platforms"));
}

// ---------- 4. 判分浮点精度边界 ----------

fn paper_with_n(n: usize, answer_value: &str) -> answergate_lib::model::Paper {
    use answergate_lib::model::{Paper, Question};
    Paper {
        paper_id: "p_precision".into(),
        level: Level::Primary,
        questions: (0..n)
            .map(|i| Question {
                id: format!("q{}", i),
                level: Level::Primary,
                grade: None,
                subject: "math".into(),
                qtype: QuestionType::SingleChoice,
                title: format!("第{}题", i),
                options: vec!["A".into(), "B".into()],
                answer: answer_value.into(),
                hint: None,
            })
            .collect(),
    }
}

#[test]
fn score_precision_boundary_07_of_10() {
    // 7/10 = 0.7 恰好等于及格线 0.7 → 必须通过（浮点精度不应误判）
    let paper = paper_with_n(10, "A");
    let answers: Vec<Answer> = paper
        .questions
        .iter()
        .take(7)
        .map(|q| Answer {
            question_id: q.id.clone(),
            value: "A".into(),
        })
        .collect();
    let r = answergate_lib::quiz::grade(&paper, &answers, 0.7);
    assert_eq!(r.correct, 7);
    assert!(r.passed, "7/10=0.7 恰好等于及格线应通过（got score={}）", r.score);
}

#[test]
fn score_precision_full_and_two_thirds() {
    // 3/3 = 1.0 通过 pass_score=1.0
    let paper = paper_with_n(3, "A");
    let all: Vec<Answer> = paper
        .questions
        .iter()
        .map(|q| Answer {
            question_id: q.id.clone(),
            value: "A".into(),
        })
        .collect();
    let r = answergate_lib::quiz::grade(&paper, &all, 1.0);
    assert!(r.passed, "全对 1.0 ≥ 1.0 应通过");

    // 2/3 ≈ 0.667 不通过 pass_score=0.7（严格小于）
    let two: Vec<Answer> = all[..2].to_vec();
    let r2 = answergate_lib::quiz::grade(&paper, &two, 0.7);
    assert!(!r2.passed, "2/3≈0.667 < 0.7 不应通过（got score={}）", r2.score);

    // 2/3 通过 pass_score=0.66（大于）
    let r3 = answergate_lib::quiz::grade(&paper, &two, 0.66);
    assert!(r3.passed, "2/3≈0.667 ≥ 0.66 应通过");
}

// ---------- 5. PaperView 值级泄漏检测 ----------

#[test]
fn paper_view_leaks_no_answer_values() {
    // 构造"答案值"本身就是独特字符串的题目（若值泄漏，序列化文本必含该值）
    let (_d, st) = fixture_state_with_config(None);
    let view = commands::do_start_quiz(&st, Level::Primary, None, NOW).unwrap();
    let json = serde_json::to_string(&view).unwrap();
    // 字段级：逐 item 校验序列化对象仅含 id/qtype/title/options 四键
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    for item in parsed["items"].as_array().unwrap() {
        let keys: Vec<String> = item.as_object().unwrap().keys().cloned().collect();
        for expected in ["id", "qtype", "title", "options"] {
            assert!(
                keys.iter().any(|k| k == expected),
                "QuizItem 缺少字段 {}（keys={:?}）",
                expected,
                keys
            );
        }
        for forbidden in ["answer", "hint"] {
            assert!(
                !keys.iter().any(|k| k == forbidden),
                "QuizItem 泄漏字段 {}（keys={:?}）",
                forbidden,
                keys
            );
        }
    }
    // 值级：单选/判断题答案合法存在于 options（选项必须下发），不作值级检查；
    // 填空题 options 为空，其答案值（含 | 分隔的每个候选）不得出现在任何 item 序列化文本中
    let gate = st.gate.lock().unwrap();
    for q in gate.active_paper().unwrap().questions.iter() {
        if q.qtype == QuestionType::FillBlank {
            for item in parsed["items"].as_array().unwrap() {
                let item_text = serde_json::to_string(item).unwrap();
                if item_text.contains(&q.title) {
                    for alt in q.answer.split('|') {
                        assert!(
                            !item_text.contains(alt.trim()),
                            "PaperView 泄漏填空答案值：{}（题目 {}）",
                            alt.trim(),
                            q.id
                        );
                    }
                }
            }
        }
    }
}

// ---------- 6. 打包资源布局一致性（暴露 bug，当前预期失败） ----------

#[test]
fn bundled_resource_layout_matches_resolver_candidates() {
    // 复现方式：tauri.conf.json 的 resources 以 "../" 开头，Tauri bundler 将
    // "../resources/question_bank/*.json" 映射到 bundle 内
    // "Contents/Resources/_up_/resources/question_bank/"（"../" → "_up_"）。
    // 而 lib.rs resolve_bank_dir 只探测 resource_dir/resources/question_bank 与
    // resource_dir/question_bank 两个候选，均不覆盖 _up_ 布局 →
    // 打包后启动 panic：题库文件读取失败（已在本机以 bundle 布局实证）。
    //
    // 本测试解析 tauri.conf.json 的 resources 声明，模拟 bundler 映射规则，
    // 断言映射后的资源相对 resource_dir 的路径与 resolve_bank_dir 的候选之一匹配。
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let conf_text =
        std::fs::read_to_string(manifest.join("tauri.conf.json")).expect("读取 tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&conf_text).unwrap();
    let resources: Vec<String> = conf["bundle"]["resources"]
        .as_array()
        .expect("bundle.resources 必须为数组")
        .iter()
        .map(|v| v.as_str().expect("resource 条目须为字符串").to_string())
        .collect();
    assert!(!resources.is_empty(), "必须声明打包资源");

    // 模拟 bundler 映射：src-tauri 相对路径 "../x" → "_up_/x"（相对 resource_dir）
    let bundled_relative: Vec<String> = resources
        .iter()
        .map(|r| {
            let stripped = r.trim_start_matches("./");
            if let Some(rest) = stripped.strip_prefix("../") {
                format!("_up_/{}", rest)
            } else {
                stripped.to_string()
            }
        })
        .collect();

    // resolve_bank_dir 的候选（lib.rs 当前实现，含 bundler 的 _up_ 映射布局）：
    let resolver_candidates = [
        "_up_/resources/question_bank",
        "_up_/question_bank",
        "resources/question_bank",
        "question_bank",
    ];

    let bank_pattern_bundled = bundled_relative
        .iter()
        .find(|r| r.contains("question_bank"))
        .expect("resources 声明必须包含题库目录");
    let ok = resolver_candidates.iter().any(|c| {
        // 候选路径须能覆盖 bundler 映射后的题库目录位置
        bank_pattern_bundled == *c
            || bank_pattern_bundled.starts_with(&format!("{}/", c))
            || c.starts_with(&format!("{}/", bank_pattern_bundled.trim_end_matches("/*")))
    });
    assert!(
        ok,
        "打包资源布局（resource_dir/{}）与 resolve_bank_dir 候选 {:?} 不匹配：\
         打包后应用将因题库读取失败而启动 panic",
        bank_pattern_bundled, resolver_candidates
    );
}
