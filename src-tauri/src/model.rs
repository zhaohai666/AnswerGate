//! 数据模型层：题库 JSON schema（与 README 4.1 一致）、试卷、判分结果、配置结构。
//!
//! 安全约定：`Question.answer/hint` 永不进入下发前端的类型；
//! 前端唯一题目来源是 `QuizItem` / `PaperView`。

use serde::{Deserialize, Serialize};

/// 学段枚举（序列化为小写字符串，与题库 JSON 的 level 字段一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Kindergarten,
    Primary,
    Junior,
}

impl Level {
    /// 全部学段（按顺序遍历用）。
    pub const ALL: [Level; 3] = [Level::Kindergarten, Level::Primary, Level::Junior];

    /// 中文展示名（前端/日志用）。
    pub fn label(self) -> &'static str {
        match self {
            Level::Kindergarten => "幼儿园",
            Level::Primary => "小学",
            Level::Junior => "初中",
        }
    }

    /// 对应的题库文件名。
    pub fn bank_file(self) -> &'static str {
        match self {
            Level::Kindergarten => "kindergarten.json",
            Level::Primary => "primary.json",
            Level::Junior => "junior.json",
        }
    }
}

/// 题型枚举（题库 JSON 的 type 字段，snake_case）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionType {
    SingleChoice,
    TrueFalse,
    FillBlank,
}

/// 题库题目（含答案与提示，仅 Rust 侧持有）。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Question {
    pub id: String,
    pub level: Level,
    /// 年级细分（如 "grade2"），可选。
    #[serde(default)]
    pub grade: Option<String>,
    pub subject: String,
    #[serde(rename = "type")]
    pub qtype: QuestionType,
    pub title: String,
    /// 单选题选项；判断/填空题为空。
    #[serde(default)]
    pub options: Vec<String>,
    /// 正确答案：填空题支持 `|` 分隔多答案。
    pub answer: String,
    #[serde(default)]
    pub hint: Option<String>,
}

impl Question {
    /// 剥离 answer/hint，生成可下发前端的题目视图。
    pub fn to_public(&self) -> QuizItem {
        QuizItem {
            id: self.id.clone(),
            qtype: self.qtype,
            title: self.title.clone(),
            options: self.options.clone(),
        }
    }
}

/// 下发前端的题目视图（无 answer/hint，防作弊约定）。
#[derive(Debug, Clone, Serialize)]
pub struct QuizItem {
    pub id: String,
    pub qtype: QuestionType,
    pub title: String,
    pub options: Vec<String>,
}

/// 整卷（Rust 侧持有，含答案）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paper {
    pub paper_id: String,
    pub level: Level,
    pub questions: Vec<Question>,
}

impl Paper {
    /// 生成整卷，paper_id 自动生成（时间戳 + 随机后缀）。
    pub fn new(level: Level, questions: Vec<Question>) -> Self {
        Paper {
            paper_id: new_paper_id(),
            level,
            questions,
        }
    }

    /// 生成下发前端的试卷视图（剥离答案）。
    pub fn to_view(&self) -> PaperView {
        PaperView {
            paper_id: self.paper_id.clone(),
            level: self.level,
            items: self.questions.iter().map(Question::to_public).collect(),
        }
    }
}

/// 生成试卷唯一 id：`p_{epoch_ms}_{hex}`。
fn new_paper_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let suffix: String = (0..6).map(|_| format!("{:x}", rng.gen_range(0..16))).collect();
    format!("p_{}_{}", chrono::Utc::now().timestamp_millis(), suffix)
}

/// 下发前端的试卷视图。
#[derive(Debug, Clone, Serialize)]
pub struct PaperView {
    pub paper_id: String,
    pub level: Level,
    pub items: Vec<QuizItem>,
}

/// 前端提交的单题作答。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub question_id: String,
    pub value: String,
}

/// 单题判分结果（仅对错，不含正确答案——降低"抄记答案"风险）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerOutcome {
    pub question_id: String,
    pub correct: bool,
}

/// 判分结果（remaining_retry 由 command 层覆写后下发）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradeResult {
    pub paper_id: String,
    pub total: u32,
    pub correct: u32,
    /// 正确率 0.0~1.0。
    pub score: f32,
    pub passed: bool,
    #[serde(default)]
    pub remaining_retry: u32,
    pub details: Vec<AnswerOutcome>,
}

/// 答题规则配置（config.toml [quiz]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuizConfig {
    #[serde(default = "default_level")]
    pub default_level: Level,
    #[serde(default = "default_question_count")]
    pub question_count: u32,
    #[serde(default = "default_pass_score")]
    pub pass_score: f32,
    #[serde(default = "default_max_retry")]
    pub max_retry: u32,
    #[serde(default = "default_cool_down_sec")]
    pub cool_down_sec: u64,
}

fn default_level() -> Level {
    Level::Primary
}
fn default_question_count() -> u32 {
    5
}
fn default_pass_score() -> f32 {
    0.8
}
fn default_max_retry() -> u32 {
    2
}
fn default_cool_down_sec() -> u64 {
    120
}

impl Default for QuizConfig {
    fn default() -> Self {
        QuizConfig {
            default_level: default_level(),
            question_count: default_question_count(),
            pass_score: default_pass_score(),
            max_retry: default_max_retry(),
            cool_down_sec: default_cool_down_sec(),
        }
    }
}

/// 目标软件白名单条目：三平台字段均可选，运行时按当前平台取值。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TargetApp {
    pub id: String,
    pub name: String,
    /// macOS .app 路径。
    #[serde(default)]
    pub mac: Option<String>,
    /// Windows exe 路径。
    #[serde(default)]
    pub win: Option<String>,
    /// Android 包名。
    #[serde(default)]
    pub android: Option<String>,
}

impl TargetApp {
    /// 按当前编译平台取启动规格（cfg! 宏运行时分支，全平台均可编译）。
    pub fn launch_spec(&self) -> Option<&str> {
        if cfg!(target_os = "macos") {
            self.mac.as_deref()
        } else if cfg!(target_os = "windows") {
            self.win.as_deref()
        } else if cfg!(target_os = "android") {
            self.android.as_deref()
        } else {
            None
        }
    }
}

/// 日志配置（config.toml [log]）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    #[serde(default = "default_true")]
    pub enable: bool,
    /// 相对 app_data_dir 的日志路径。
    #[serde(default = "default_log_path")]
    pub path: String,
}

fn default_true() -> bool {
    true
}
fn default_log_path() -> String {
    "logs/answer_gate.log".to_string()
}

impl Default for LogConfig {
    fn default() -> Self {
        LogConfig {
            enable: true,
            path: default_log_path(),
        }
    }
}

/// 家长密码配置（config.toml [parent]，P1-01 默认关闭）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParentConfig {
    /// SHA-256 hex；None/空 表示未启用应急解锁。
    #[serde(default)]
    pub password_hash: Option<String>,
}

/// [monitor] 段配置（v2 T01 预留 fast_test；完整字段 min_use_minutes 等在 T02 扩展）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MonitorConfig {
    /// true 时轮次固定 10 秒并忽略 use_minutes（U10 加速档，QA 用；前端显示"测试模式"水印）。
    #[serde(default)]
    pub fast_test: bool,
}

/// 应用配置总入口。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub quiz: QuizConfig,
    /// config.toml 中为 [[target_apps]] 数组。
    #[serde(default)]
    pub target_apps: Vec<TargetApp>,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub parent: ParentConfig,
    /// config.toml 中为 [monitor] 段。
    #[serde(default)]
    pub monitor: MonitorConfig,
}
