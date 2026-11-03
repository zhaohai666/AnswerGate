//! 题库加载/校验、过滤、随机抽题（去重、移动端过滤填空）、判分。
//!
//! 判分细则（共享知识 U3）：trim + 全角转半角 + 忽略大小写后精确匹配；
//! 填空题 answer 字段支持 `|` 分隔多答案，命中任一即正确。

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use rand::seq::SliceRandom;

use crate::error::AppError;
use crate::model::{
    Answer, AnswerOutcome, GradeResult, Level, Paper, Question, QuestionType,
};

/// (学段, 题库文件名) 对照表。
pub const LEVEL_BANK_FILES: [(Level, &str); 3] = [
    (Level::Kindergarten, "kindergarten.json"),
    (Level::Primary, "primary.json"),
    (Level::Junior, "junior.json"),
];

/// 加载全部题库（三个 JSON），做完整 schema 校验。
pub fn load_bank(bank_dir: &Path) -> Result<Vec<Question>, AppError> {
    let mut all: Vec<Question> = Vec::new();
    for (level, file) in LEVEL_BANK_FILES {
        let path = bank_dir.join(file);
        let text = fs::read_to_string(&path).map_err(|e| {
            AppError::QuestionBank(format!("题库文件读取失败 {}: {}", path.display(), e))
        })?;
        let questions: Vec<Question> = serde_json::from_str(&text).map_err(|e| {
            AppError::QuestionBank(format!("题库 JSON 解析失败（{}）：{}", file, e))
        })?;
        validate_questions(&questions, Some(level))
            .map_err(|e| AppError::QuestionBank(format!("{}：{}", file, e)))?;
        all.extend(questions);
    }
    if all.is_empty() {
        return Err(AppError::QuestionBank("题库为空：三个学段均无题目".to_string()));
    }
    Ok(all)
}

/// 校验一组题目：字段完整、答案与题型匹配、id 唯一、level 与所属文件一致。
pub fn validate_questions(
    questions: &[Question],
    expected_level: Option<Level>,
) -> Result<(), AppError> {
    let mut seen: HashSet<&str> = HashSet::new();
    for q in questions {
        // id 唯一性
        if q.id.trim().is_empty() {
            return Err(AppError::QuestionBank(format!(
                "题目缺少 id：{}",
                q.title
            )));
        }
        if !seen.insert(q.id.as_str()) {
            return Err(AppError::QuestionBank(format!("题目 id 重复：{}", q.id)));
        }
        // 字段完整
        if q.title.trim().is_empty() {
            return Err(AppError::QuestionBank(format!("题目题干为空：{}", q.id)));
        }
        if q.subject.trim().is_empty() {
            return Err(AppError::QuestionBank(format!("题目学科为空：{}", q.id)));
        }
        if let Some(exp) = expected_level {
            if q.level != exp {
                return Err(AppError::QuestionBank(format!(
                    "题目 {} 的 level（{}）与所属文件不符（应为 {}）",
                    q.id,
                    q.level.label(),
                    exp.label()
                )));
            }
        }
        // 答案合法性
        match q.qtype {
            QuestionType::SingleChoice => {
                if q.options.is_empty() {
                    return Err(AppError::QuestionBank(format!(
                        "单选题 {} 缺少选项",
                        q.id
                    )));
                }
                if !q.options.contains(&q.answer) {
                    return Err(AppError::QuestionBank(format!(
                        "单选题 {} 的答案（{}）不在选项中",
                        q.id, q.answer
                    )));
                }
            }
            QuestionType::TrueFalse => {
                if !matches!(q.answer.as_str(), "对" | "错") {
                    return Err(AppError::QuestionBank(format!(
                        "判断题 {} 的答案（{}）非法，应为「对」或「错」",
                        q.id, q.answer
                    )));
                }
            }
            QuestionType::FillBlank => {
                if q.answer.trim().is_empty() {
                    return Err(AppError::QuestionBank(format!(
                        "填空题 {} 缺少答案",
                        q.id
                    )));
                }
            }
        }
    }
    Ok(())
}

/// 按学段/年级/学科过滤题库。
pub fn filter_pool(
    bank: &[Question],
    level: Level,
    grade: Option<&str>,
    subject: Option<&str>,
) -> Vec<Question> {
    bank.iter()
        .filter(|q| q.level == level)
        .filter(|q| match grade {
            Some(g) => q.grade.as_deref() == Some(g),
            None => true,
        })
        .filter(|q| match subject {
            Some(s) => q.subject == s,
            None => true,
        })
        .cloned()
        .collect()
}

/// 随机抽题（洗牌后取前 N，天然去重）。
///
/// 返回 `(题目, 是否降级)`：题量不足时取全部并降级标记（架构默认决策 U-抽题回退）；
/// 过滤后为空则返回 `NotEnoughQuestions`。
pub fn pick_questions(
    mut pool: Vec<Question>,
    count: usize,
    exclude_fill_blank: bool,
) -> Result<(Vec<Question>, bool), AppError> {
    if exclude_fill_blank {
        // 移动端适配：触屏输入不便，抽题时过滤填空题（PRD P0-02/P0-11）
        pool.retain(|q| q.qtype != QuestionType::FillBlank);
    }
    if pool.is_empty() {
        return Err(AppError::NotEnoughQuestions {
            requested: count as u32,
            available: 0,
        });
    }
    let mut rng = rand::thread_rng();
    pool.shuffle(&mut rng);
    let degraded = pool.len() < count;
    let take = count.min(pool.len());
    Ok((pool.into_iter().take(take).collect(), degraded))
}

/// 组卷：过滤 + 抽题 + 生成 Paper（含答案，仅 Rust 侧持有）。
pub fn make_paper(
    bank: &[Question],
    level: Level,
    count: usize,
    exclude_fill_blank: bool,
) -> Result<Paper, AppError> {
    let pool = filter_pool(bank, level, None, None);
    let (questions, _degraded) = pick_questions(pool, count, exclude_fill_blank)?;
    Ok(Paper::new(level, questions))
}

/// 答案归一化：trim + 全角转半角 + 忽略大小写。
pub fn normalize_answer(input: &str) -> String {
    input
        .trim()
        .chars()
        .map(|c| {
            if c == '\u{3000}' {
                ' ' // 全角空格
            } else if ('\u{FF01}'..='\u{FF5E}').contains(&c) {
                // 全角 ASCII 区 → 半角
                char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
            } else {
                c
            }
        })
        .collect::<String>()
        .to_lowercase()
}

/// 单题判分：填空题按 `|` 多答案任一命中即正确。
pub fn is_correct(question: &Question, value: &str) -> bool {
    let norm = normalize_answer(value);
    if norm.is_empty() {
        return false;
    }
    match question.qtype {
        QuestionType::FillBlank => question
            .answer
            .split('|')
            .any(|alt| normalize_answer(alt) == norm),
        QuestionType::SingleChoice | QuestionType::TrueFalse => {
            normalize_answer(&question.answer) == norm
        }
    }
}

/// 整卷判分：Rust 侧权威计算，正确率 ≥ pass_score 即通过。
pub fn grade(paper: &Paper, answers: &[Answer], pass_score: f32) -> GradeResult {
    let by_id: std::collections::HashMap<&str, &str> = answers
        .iter()
        .map(|a| (a.question_id.as_str(), a.value.as_str()))
        .collect();
    let mut details: Vec<AnswerOutcome> = Vec::with_capacity(paper.questions.len());
    let mut correct: u32 = 0;
    for q in &paper.questions {
        // 未作答按错误处理
        let given = by_id.get(q.id.as_str()).copied().unwrap_or("");
        let ok = is_correct(q, given);
        if ok {
            correct += 1;
        }
        details.push(AnswerOutcome {
            question_id: q.id.clone(),
            correct: ok,
        });
    }
    let total = paper.questions.len() as u32;
    let score = if total == 0 {
        0.0
    } else {
        correct as f32 / total as f32
    };
    GradeResult {
        paper_id: paper.paper_id.clone(),
        total,
        correct,
        score,
        passed: total > 0 && score + f32::EPSILON >= pass_score,
        remaining_retry: 0,
        details,
    }
}
