//! 题库内容校验（CI 自动跑，PRD P0-01 / 阶段5 验收）：
//! 遍历 resources/question_bank/ 三个 JSON，校验：
//! - JSON 可解析、字段符合 schema（复用 quiz::validate_questions）
//! - id 唯一（含跨文件）、level 与所属文件一致、答案合法
//! - 当前为空占位文件时跳过并提示（T05 填充后自动全量校验）

use std::collections::HashSet;
use std::path::PathBuf;

use answergate_lib::model::{Level, Question};
use answergate_lib::quiz;

const BANK_FILES: [(&str, Level); 3] = [
    ("kindergarten.json", Level::Kindergarten),
    ("primary.json", Level::Primary),
    ("junior.json", Level::Junior),
];

fn bank_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources/question_bank")
}

#[test]
fn check_builtin_question_banks() {
    let dir = bank_dir();
    assert!(dir.exists(), "题库目录不存在：{}", dir.display());

    let mut all_ids: HashSet<String> = HashSet::new();
    let mut total = 0usize;

    for (file, level) in BANK_FILES {
        let path = dir.join(file);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("题库文件读取失败 {}: {}", path.display(), e));
        let questions: Vec<Question> = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("题库 JSON 解析失败（{}）：{}", file, e));

        if questions.is_empty() {
            println!("[check_bank] {} 为空占位（T05 待填充），跳过内容校验", file);
            continue;
        }

        quiz::validate_questions(&questions, Some(level))
            .unwrap_or_else(|e| panic!("{} 校验失败：{}", file, e));

        // 跨文件 id 唯一性
        for q in &questions {
            assert!(
                all_ids.insert(q.id.clone()),
                "{}：题目 id 跨文件重复：{}",
                file,
                q.id
            );
        }
        println!("[check_bank] {} 通过校验：{} 题", file, questions.len());
        total += questions.len();
    }

    println!("[check_bank] 题库总量：{} 题", total);
}
