//! Tauri command 层：前端可调用的全部能力（错误统一 AppError → {code, message, data}）。
//!
//! 核心约定：
//! - 判分权威在 Rust，PaperView 剥离 answer/hint；
//! - 状态机在 GateState，持久化独立于 config.toml；
//! - 冷却倒计时由 Rust 异步线程每秒 emit `gate://cooldown-tick`，归零 emit
//!   `gate://cooldown-finished`；
//! - 逻辑函数（do_*）与 command 薄封装分离，便于集成测试。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::config;
use crate::error::AppError;
use crate::gate::{GatePhase, GateState};
use crate::model::{Answer, AppConfig, GradeResult, Level, PaperView, Question, TargetApp};
use crate::quiz;
use crate::unlock;

/// 默认配置模板（编译期内嵌，首启写入 app_data_dir/config.toml）。
pub const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../../resources/config.toml");

/// 应用共享状态（lib.rs setup 时 manage）。
pub struct AppState {
    pub config: AppConfig,
    pub bank: Vec<Question>,
    pub gate: Mutex<GateState>,
    pub app_data_dir: PathBuf,
}

impl AppState {
    /// 构建状态：首启写默认配置模板 → 读取配置 → 加载题库 → 恢复门禁状态。
    pub fn new(app_data_dir: PathBuf, bank_dir: &Path) -> Result<Self, AppError> {
        std::fs::create_dir_all(&app_data_dir).map_err(|e| {
            AppError::Storage(format!("创建数据目录失败 {}: {}", app_data_dir.display(), e))
        })?;
        let config_path = app_data_dir.join("config.toml");
        if !config_path.exists() {
            std::fs::write(&config_path, DEFAULT_CONFIG_TEMPLATE).map_err(|e| {
                AppError::Storage(format!("写默认配置失败 {}: {}", config_path.display(), e))
            })?;
            tracing::info!("首启：已写入默认配置模板 {}", config_path.display());
        }
        let cfg = config::load_or_default(&config_path);
        let bank = quiz::load_bank(bank_dir)?;
        let gate = Mutex::new(GateState::recover(
            &Self::state_file_of(&app_data_dir),
            cfg.quiz.max_retry,
            now(),
        ));
        Ok(AppState {
            config: cfg,
            bank,
            gate,
            app_data_dir,
        })
    }

    fn state_file_of(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join(crate::gate::STATE_FILE_NAME)
    }

    /// 门禁状态文件路径。
    pub fn state_file(&self) -> PathBuf {
        Self::state_file_of(&self.app_data_dir)
    }
}

/// 当前 Unix epoch 秒。
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---------- 下发视图 ----------

/// 规则摘要（学段选择页底部文案）。
#[derive(Debug, Clone, Serialize)]
pub struct RulesSummary {
    pub question_count: u32,
    pub pass_score: f32,
    pub max_retry: u32,
    pub cool_down_sec: u64,
    pub default_level: Level,
    pub levels: Vec<LevelEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LevelEntry {
    pub level: Level,
    pub label: &'static str,
}

/// 目标软件视图（含当前平台可用性，供前端跳过不可用项）。
#[derive(Debug, Clone, Serialize)]
pub struct TargetAppView {
    pub id: String,
    pub name: String,
    pub available: bool,
}

/// 门禁状态视图（前端唯一可读状态，共享知识 #3）。
#[derive(Debug, Clone, Serialize)]
pub struct GateStateView {
    pub phase: GatePhase,
    pub remaining_retry: u32,
    pub cooldown_remaining_sec: u64,
    pub selected_target_name: Option<String>,
    pub current_level: Option<Level>,
}

// ---------- 核心逻辑（&AppState，测试友好） ----------

/// 规则摘要逻辑。
pub fn rules_summary(state: &AppState) -> RulesSummary {
    RulesSummary {
        question_count: state.config.quiz.question_count,
        pass_score: state.config.quiz.pass_score,
        max_retry: state.config.quiz.max_retry,
        cool_down_sec: state.config.quiz.cool_down_sec,
        default_level: state.config.quiz.default_level,
        levels: Level::ALL
            .iter()
            .map(|l| LevelEntry {
                level: *l,
                label: l.label(),
            })
            .collect(),
    }
}

/// 目标软件列表逻辑。
pub fn list_target_views(state: &AppState) -> Vec<TargetAppView> {
    state
        .config
        .target_apps
        .iter()
        .map(|t| TargetAppView {
            id: t.id.clone(),
            name: t.name.clone(),
            available: t.launch_spec().is_some(),
        })
        .collect()
}

/// 开始答题：校验目标 → 抽题（Android 过滤填空）→ QUIZ → 持久化 → 下发无答案试卷。
pub fn do_start_quiz(
    state: &AppState,
    level: Level,
    target_id: Option<String>,
    now_ts: i64,
) -> Result<PaperView, AppError> {
    // 1) 目标软件校验
    if let Some(tid) = &target_id {
        if !state.config.target_apps.iter().any(|t| &t.id == tid) {
            return Err(AppError::TargetNotFound(tid.clone()));
        }
    }
    // 2) 抽题（题量不足回退取全部；空题池报 NotEnoughQuestions）
    let count = state.config.quiz.question_count as usize;
    let exclude_fill_blank = cfg!(target_os = "android"); // 移动端触屏输入不便（P0-11）
    let paper = quiz::make_paper(&state.bank, level, count, exclude_fill_blank)?;
    // 3) 状态机 QUIZ（冷却中在此被拒绝，状态不被污染）
    let mut gate = state.gate.lock().unwrap();
    gate.begin_quiz(level, target_id, now_ts)?;
    gate.set_active_paper(paper.clone());
    gate.save(&state.state_file())?;
    tracing::info!(event = "quiz_start", level = %level.label(), paper_id = %paper.paper_id, "开始答题");
    Ok(paper.to_view())
}

/// 失败后「再试一次」：校验余额 → 重新抽题 → QUIZ 保持 → 持久化。
pub fn do_retry_quiz(state: &AppState, now_ts: i64) -> Result<PaperView, AppError> {
    let mut gate = state.gate.lock().unwrap();
    gate.begin_retry(now_ts)?;
    let level = gate.current_level.ok_or_else(|| {
        AppError::InvalidInput("无进行中学段，无法重试（请从学段选择开始）".to_string())
    })?;
    let count = state.config.quiz.question_count as usize;
    let exclude_fill_blank = cfg!(target_os = "android");
    let paper = quiz::make_paper(&state.bank, level, count, exclude_fill_blank)?;
    gate.set_active_paper(paper.clone());
    gate.save(&state.state_file())?;
    tracing::info!(event = "quiz_start", level = %level.label(), paper_id = %paper.paper_id, retry = true, "重试答题");
    Ok(paper.to_view())
}

/// 提交整卷 → Rust 权威判分 → 状态迁移 → 持久化 → 记日志。
///
/// FAIL 且重试耗尽时状态机自动进入 COOLDOWN（冷却计时由调用方 spawn ticker）。
pub fn do_submit_answers(
    state: &AppState,
    paper_id: &str,
    answers: &[Answer],
    now_ts: i64,
) -> Result<GradeResult, AppError> {
    let mut gate = state.gate.lock().unwrap();
    let paper = match gate.active_paper() {
        Some(p) if p.paper_id == paper_id => p.clone(),
        _ => return Err(AppError::PaperNotFound(paper_id.to_string())),
    };
    let mut result = quiz::grade(&paper, answers, state.config.quiz.pass_score);
    let cool_down_sec = state.config.quiz.cool_down_sec;
    let phase = gate.record_result(result.passed, cool_down_sec, now_ts);
    result.remaining_retry = gate.remaining_retry;
    gate.save(&state.state_file())?;
    tracing::info!(
        event = "grade",
        level = %paper.level.label(),
        paper_id = %paper.paper_id,
        score = result.score,
        result = if result.passed { "pass" } else { "fail" },
        phase = ?phase,
        "判分完成"
    );
    Ok(result)
}

/// 解锁启动目标软件（PASS 后或家长密码应急）。
///
/// 成功记日志并延迟 1s 调用 app_handle.exit(0)，给前端留出结果页展示时间。
pub fn do_launch_target(
    state: &AppState,
    app: &AppHandle,
    force: bool,
    now_ts: i64,
) -> Result<(), AppError> {
    {
        let gate = state.gate.lock().unwrap();
        if !force && gate.phase != GatePhase::Passed {
            return Err(AppError::InvalidInput(
                "未通过答题校验，不能启动目标软件".to_string(),
            ));
        }
    }
    // 选目标：优先 selected_target_id；未选且当前平台仅一个可用目标时自动选中
    let target: TargetApp = {
        let targets = &state.config.target_apps;
        let selected = state.gate.lock().unwrap().selected_target_id.clone();
        match selected.as_deref() {
            Some(id) => targets
                .iter()
                .find(|t| t.id == id)
                .cloned()
                .ok_or_else(|| AppError::TargetNotFound(id.to_string()))?,
            None => {
                let available: Vec<&TargetApp> =
                    targets.iter().filter(|t| t.launch_spec().is_some()).collect();
                match available.len() {
                    1 => available[0].clone(),
                    _ => {
                        return Err(AppError::TargetNotFound(
                            "未选择目标软件，且当前平台可用目标不是唯一".to_string(),
                        ))
                    }
                }
            }
        }
    };
    unlock::launch_target(&target, app)?;
    let _ = now_ts;
    tracing::info!(event = "unlock", result = "launch_ok", target = %target.name, "解锁启动成功");
    // 延迟退出，让前端展示"已启动，即将关闭"
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(1));
        handle.exit(0);
    });
    Ok(())
}

/// 家长密码校验（纯逻辑，P1-01；config 未启用或密码错误一律 ParentPasswordError）。
pub fn verify_parent_password(state: &AppState, password: &str) -> Result<(), AppError> {
    let expected = state
        .config
        .parent
        .password_hash
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty());
    let expected = match expected {
        Some(h) => h,
        None => {
            tracing::warn!(event = "parent_unlock_fail", "家长密码未启用");
            return Err(AppError::ParentPasswordError);
        }
    };
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    if actual != expected.to_lowercase() {
        tracing::warn!(event = "parent_unlock_fail", "家长密码错误");
        return Err(AppError::ParentPasswordError);
    }
    Ok(())
}

/// 家长密码应急解锁（P1-01，config 默认关闭）。
pub fn do_parent_unlock(
    state: &AppState,
    app: &AppHandle,
    password: &str,
) -> Result<(), AppError> {
    verify_parent_password(state, password)?;
    tracing::warn!(event = "parent_unlock", result = "pass", "家长密码应急解锁");
    do_launch_target(state, app, true, now())
}

/// 门禁状态视图（含重启恢复；冷却到点自动复位）。
pub fn gate_view(state: &AppState, now_ts: i64) -> GateStateView {
    let mut gate = state.gate.lock().unwrap();
    gate.finish_cooldown_if_elapsed(now_ts);
    let selected_target_name = gate
        .selected_target_id
        .as_deref()
        .and_then(|id| state.config.target_apps.iter().find(|t| t.id == id))
        .map(|t| t.name.clone());
    GateStateView {
        phase: gate.phase,
        remaining_retry: gate.remaining_retry,
        cooldown_remaining_sec: gate.cooldown_remaining(now_ts).unwrap_or(0),
        selected_target_name,
        current_level: gate.current_level,
    }
}

/// 冷却倒计时线程：每秒 emit `gate://cooldown-tick {remaining}`，归零 emit
/// `gate://cooldown-finished` 并持久化复位（U5：Rust 计时权威）。
pub fn spawn_cooldown_ticker(app: AppHandle) {
    std::thread::spawn(move || loop {
        let ts = now();
        let remaining = {
            let state = app.state::<AppState>();
            let mut gate = state.gate.lock().unwrap();
            gate.finish_cooldown_if_elapsed(ts);
            gate.cooldown_remaining(ts)
        };
        match remaining {
            Some(sec) => {
                let _ = app.emit("gate://cooldown-tick", serde_json::json!({ "remaining": sec }));
                std::thread::sleep(Duration::from_secs(1));
            }
            None => {
                let _ = app.emit("gate://cooldown-finished", serde_json::json!({}));
                let state = app.state::<AppState>();
                let _ = state.gate.lock().unwrap().save(&state.state_file());
                tracing::info!(event = "cooldown_finished", "冷却结束");
                break;
            }
        }
    });
}

// ---------- Tauri command 薄封装 ----------

/// 获取规则摘要（题量/及格线/重试/冷却 + 学段列表）。
#[tauri::command]
pub fn get_app_config(state: State<AppState>) -> Result<RulesSummary, AppError> {
    Ok(rules_summary(&state))
}

/// 白名单目标软件列表（多目标时前端展示选择页；单目标自动跳过）。
#[tauri::command]
pub fn list_targets(state: State<AppState>) -> Result<Vec<TargetAppView>, AppError> {
    Ok(list_target_views(&state))
}

/// 开始答题（错误：CooldownActive / NotEnoughQuestions / TargetNotFound）。
#[tauri::command]
pub fn start_quiz(
    state: State<AppState>,
    level: Level,
    target_id: Option<String>,
) -> Result<PaperView, AppError> {
    do_start_quiz(&state, level, target_id, now())
}

/// 失败后重试（错误：NoRetryLeft / CooldownActive）。
#[tauri::command]
pub fn retry_quiz(state: State<AppState>) -> Result<PaperView, AppError> {
    do_retry_quiz(&state, now())
}

/// 提交整卷判分（错误：PaperNotFound）。FAIL 且重试耗尽自动入 COOLDOWN。
#[tauri::command]
pub fn submit_answers(
    state: State<AppState>,
    app_handle: AppHandle,
    paper_id: String,
    answers: Vec<Answer>,
) -> Result<GradeResult, AppError> {
    let result = do_submit_answers(&state, &paper_id, &answers, now())?;
    if state.gate.lock().unwrap().phase == GatePhase::Cooldown {
        spawn_cooldown_ticker(app_handle);
    }
    Ok(result)
}

/// 解锁启动目标软件（PASS 后调用；成功后 1s 退出应用）。
#[tauri::command]
pub fn launch_target(state: State<AppState>, app_handle: AppHandle) -> Result<(), AppError> {
    do_launch_target(&state, &app_handle, false, now())
}

/// 启动恢复：读取持久化门禁状态；冷却中返回剩余秒数（前端直接进冷却页）。
#[tauri::command]
pub fn get_gate_state(state: State<AppState>) -> Result<GateStateView, AppError> {
    Ok(gate_view(&state, now()))
}

/// 家长密码应急解锁（P1-01，config 默认关闭）。
#[tauri::command]
pub fn parent_unlock(
    state: State<AppState>,
    app_handle: AppHandle,
    password: String,
) -> Result<(), AppError> {
    do_parent_unlock(&state, &app_handle, &password)
}
