//! Tauri command 层：前端可调用的全部能力（错误统一 AppError → {code, message, data}）。
//!
//! 核心约定：
//! - 判分权威在 Rust，PaperView 剥离 answer/hint；
//! - 状态机在 GateState，持久化独立于 config.toml；
//! - 冷却倒计时由 Rust 异步线程每秒 emit `gate://cooldown-tick`，归零 emit
//!   `gate://cooldown-finished`；
//! - v2 使用中倒计时由 Rust 线程每秒 emit `gate://using-tick`，到点 kill + 置顶 +
//!   emit `gate://timeout-reached` 并自动进入超时答题（QuizReason=Timeup）；
//! - 逻辑函数（do_*）与 command 薄封装分离，便于集成测试。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::config;
use crate::error::AppError;
use crate::gate::{GatePhase, GateState, QuizReason};
use crate::model::{Answer, AppConfig, GradeResult, Level, PaperView, Question, TargetApp};
use crate::monitor;
use crate::quiz;
use crate::unlock;

/// 默认配置模板（编译期内嵌，首启写入 app_data_dir/config.toml）。
pub const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../../resources/config.toml");

/// 使用中每秒 tick 的警告阈值（剩余 ≤60s 前端变红，P1-08）。
const WARNING_THRESHOLD_SEC: i64 = 60;

/// 应用共享状态（lib.rs setup 时 manage）。
pub struct AppState {
    pub config: AppConfig,
    pub bank: Vec<Question>,
    pub gate: Mutex<GateState>,
    pub app_data_dir: PathBuf,
    /// 使用中监控任务的停止信号（stop_monitor 置位，监控线程检测后退出）。
    pub monitor_stop: Arc<AtomicBool>,
}

impl AppState {
    /// 构建状态：首启写默认配置模板 → 读取配置 → 加载题库 → 恢复门禁状态
    /// → Using 态重验（进程存活 + 剩余时长，防杀 AnswerGate 绕计时）。
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
            monitor_stop: Arc::new(AtomicBool::new(false)),
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
    /// 当前答题原因（v2：前端据 Timeup 区分续期文案）。
    pub quiz_reason: QuizReason,
    /// 已用轮次数（v2）。
    pub round_count: u32,
}

/// 使用中页数据视图（v2：get_monitor_state 下发）。
#[derive(Debug, Clone, Serialize)]
pub struct MonitorStateView {
    pub phase: GatePhase,
    pub managed_target_name: Option<String>,
    pub remaining_sec: i64,
    pub rounds_used: u32,
    pub warning: bool,
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

/// 计算纳管目标的单轮时长（秒）：fast_test=true 固定 10 秒（U10），
/// 否则取目标 use_minutes；目标未知回退默认 10 分钟。
pub fn round_sec_for(state: &AppState, target_id: Option<&str>) -> i64 {
    let use_minutes = target_id
        .and_then(|id| state.config.target_apps.iter().find(|t| t.id == id))
        .map(|t| t.use_minutes)
        .unwrap_or(10.0);
    monitor::round_duration_sec(use_minutes, state.config.monitor.fast_test)
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
    // 3) 状态机 QUIZ（冷却中/使用中在此被拒绝，状态不被污染）
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
/// v2：quiz_reason 随判分落入 Passed{Initial|Timeup}——Timeup 通过后走续期拉起。
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
    let reason = gate.quiz_reason;
    let mut result = quiz::grade(&paper, answers, state.config.quiz.pass_score);
    let cool_down_sec = state.config.quiz.cool_down_sec;
    let phase = gate.record_result(result.passed, reason, cool_down_sec, now_ts);
    result.remaining_retry = gate.remaining_retry;
    gate.save(&state.state_file())?;
    tracing::info!(
        event = "grade",
        level = %paper.level.label(),
        paper_id = %paper.paper_id,
        score = result.score,
        result = if result.passed { "pass" } else { "fail" },
        reason = ?reason,
        phase = ?phase,
        "判分完成"
    );
    Ok(result)
}

/// 按句柄结果进入 Using（纯逻辑，便于测试）。
///
/// - `pid=None`（macOS 5s 窗口取不到句柄，U7）：abandon_round 回 Idle，
///   报 MonitorFailed——宁可不管控不误杀；
/// - `Some(pid)`：按 quiz_reason 走 begin_using（Initial）或 extend_round（Timeup）；
/// - 轮次上限：非 force 且 max_rounds>0 且续期超限 → RoundLimitReached（状态不变）。
pub fn enter_using_with_handle(
    gate: &mut GateState,
    target_id: &str,
    pid: Option<i32>,
    force: bool,
    max_rounds: u32,
    now_ts: i64,
) -> Result<(), AppError> {
    let Some(pid) = pid else {
        gate.abandon_round();
        tracing::warn!(event = "round_abandoned", reason = "handle_lost", target = %target_id, "未取得进程句柄，放弃纳管（宁不管控不误杀）");
        return Err(AppError::MonitorFailed(
            "目标软件已启动，但未能取得进程句柄，本轮不纳管".to_string(),
        ));
    };
    if !force
        && gate.quiz_reason == QuizReason::Timeup
        && max_rounds > 0
        && gate.round_count >= max_rounds
    {
        return Err(AppError::RoundLimitReached { max_rounds });
    }
    if gate.quiz_reason == QuizReason::Timeup {
        gate.extend_round(target_id, pid, now_ts)
    } else {
        gate.begin_using(target_id, pid, now_ts, force)
    }
}

/// 解锁启动目标软件（v2 语义：PASS 后纳管启动，不再 exit(0)）。
///
/// 流程：相位校验 → 选目标 → launch+取句柄 → enter_using/extend_round →
/// 持久化 → spawn 使用中监控线程。
/// - launch 失败：状态不变（仍 Passed），可直接重试；
/// - 取不到句柄：abandon_round 回 Idle + MonitorFailed（U7 宁不管控不误杀）。
pub fn do_launch_target(
    state: &AppState,
    app: &AppHandle,
    force: bool,
    now_ts: i64,
) -> Result<(), AppError> {
    {
        let gate = state.gate.lock().unwrap();
        if gate.phase == GatePhase::Using {
            return Err(AppError::AlreadyManaged);
        }
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
    // 启动 + 取句柄（Err = 启动本身失败，状态不变）
    let pid = unlock::launch_target_handle(&target, app)?;
    let max_rounds = state.config.monitor.max_rounds;
    {
        let mut gate = state.gate.lock().unwrap();
        enter_using_with_handle(&mut gate, &target.id, pid, force, max_rounds, now_ts)?;
        gate.save(&state.state_file())?;
    }
    tracing::info!(event = "round_start", target = %target.name, pid = pid.unwrap_or(0), "纳管启动成功");
    let round_sec = round_sec_for(state, Some(&target.id));
    spawn_using_monitor(app.clone(), round_sec);
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

/// 家长密码应急解锁（P1-01，config 默认关闭）。v2：同样纳管启动，不再退出应用。
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
        quiz_reason: gate.quiz_reason,
        round_count: gate.round_count,
    }
}

/// 使用中页数据（仅 Using 态可调，否则 NotUsing）。
pub fn do_monitor_view(state: &AppState, now_ts: i64) -> Result<MonitorStateView, AppError> {
    let gate = state.gate.lock().unwrap();
    if gate.phase != GatePhase::Using {
        return Err(AppError::NotUsing);
    }
    let round_sec = round_sec_for(state, gate.managed_target_id.as_deref());
    let remaining_sec = gate.round_remaining_sec(now_ts, round_sec).unwrap_or(0);
    let managed_target_name = gate
        .managed_target_id
        .as_deref()
        .and_then(|id| state.config.target_apps.iter().find(|t| t.id == id))
        .map(|t| t.name.clone());
    Ok(MonitorStateView {
        phase: gate.phase,
        managed_target_name,
        remaining_sec,
        rounds_used: gate.round_count,
        warning: remaining_sec <= WARNING_THRESHOLD_SEC,
    })
}

// ---------- 使用中监控（v2 计时权威：Rust 每秒 tick） ----------

/// 监控单步判定结果（纯逻辑，供监控线程与测试复用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitorTick {
    /// 继续计时（携带剩余秒与警告位）。
    Tick { remaining_sec: i64, warning: bool },
    /// 倒计时归零：调用方 kill + 置顶 + 进入超时答题。
    Timeout,
    /// 纳管进程已死亡：调用方 abandon_round 回 Idle（不触发答题，C13）。
    ProcessExited,
    /// 已不在 Using（被 stop_monitor/其他流程改走）：监控线程直接退出。
    Stopped,
}

/// 监控单步：存活检查 + 剩余时长判定。
pub fn monitor_step(gate: &mut GateState, round_sec: i64, now_ts: i64) -> MonitorTick {
    if gate.phase != GatePhase::Using {
        return MonitorTick::Stopped;
    }
    let Some(pid) = gate.managed_pid else {
        return MonitorTick::ProcessExited;
    };
    if !monitor::alive(pid) {
        return MonitorTick::ProcessExited;
    }
    let remaining = gate.round_remaining_sec(now_ts, round_sec).unwrap_or(0);
    if remaining <= 0 {
        MonitorTick::Timeout
    } else {
        MonitorTick::Tick {
            remaining_sec: remaining,
            warning: remaining <= WARNING_THRESHOLD_SEC,
        }
    }
}

/// 重启恢复重验（装配层调用）：Using 态校验进程存活与剩余时长，
/// 失效则 abandon_round 回 Idle 并持久化。返回是否恢复纳管成功。
pub fn revalidate_using_on_startup(state: &AppState, now_ts: i64) -> bool {
    let target_id = state.gate.lock().unwrap().managed_target_id.clone();
    let round_sec = round_sec_for(state, target_id.as_deref());
    let mut gate = state.gate.lock().unwrap();
    if gate.phase != GatePhase::Using {
        return false;
    }
    let alive_ok = gate.managed_pid.map(monitor::alive).unwrap_or(false);
    let remaining_ok = gate.round_remaining_sec(now_ts, round_sec).unwrap_or(0) > 0;
    if alive_ok && remaining_ok {
        tracing::info!(event = "round_resume", pid = gate.managed_pid.unwrap_or(0), "重启恢复：继续本轮计时");
        return true;
    }
    gate.abandon_round();
    let _ = gate.save(&state.state_file());
    tracing::warn!(event = "round_abandoned", reason = "handle_lost", "重启恢复：句柄失效或本轮已超时，回 Idle");
    false
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

/// 使用中监控线程：每秒 tick（存活检查 + 倒计时 emit）。
///
/// - `gate://using-tick {remaining_sec, warning}`：每秒（架构共享知识 #2）；
/// - 到点：terminate(pid) → AnswerGate 窗口置顶 → 自动进入超时答题
///   （QuizReason=Timeup，不消耗重试余额）→ emit `gate://timeout-reached`；
/// - 进程意外退出：abandon_round 回 Idle → emit `gate://process-exited`（C13）；
/// - 停止：stop_monitor 置位 monitor_stop 或相位离开 Using → 线程退出。
pub fn spawn_using_monitor(app: AppHandle, round_sec: i64) {
    let stop_flag = app.state::<AppState>().monitor_stop.clone();
    stop_flag.store(false, Ordering::SeqCst);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        if stop_flag.load(Ordering::SeqCst) {
            tracing::info!(event = "monitor_stopped", "监控线程收到停止信号");
            break;
        }
        let state = app.state::<AppState>();
        let now_ts = now();
        let tick = {
            let mut gate = state.gate.lock().unwrap();
            monitor_step(&mut gate, round_sec, now_ts)
        };
        match tick {
            MonitorTick::Tick {
                remaining_sec,
                warning,
            } => {
                let _ = app.emit(
                    "gate://using-tick",
                    serde_json::json!({ "remaining_sec": remaining_sec, "warning": warning }),
                );
            }
            MonitorTick::Stopped => {
                tracing::info!(event = "monitor_stopped", "相位已离开 Using，监控线程退出");
                break;
            }
            MonitorTick::ProcessExited => {
                let state = app.state::<AppState>();
                {
                    let mut gate = state.gate.lock().unwrap();
                    gate.abandon_round();
                    let _ = gate.save(&state.state_file());
                }
                let _ = app.emit("gate://process-exited", serde_json::json!({}));
                tracing::warn!(event = "round_abandoned", reason = "process_exit", "纳管进程意外退出，回 Idle");
                break;
            }
            MonitorTick::Timeout => {
                handle_timeout(&app, &state, round_sec, now_ts);
                break;
            }
        }
    });
}

/// 超时收尾：kill → 置顶 → 自动进入超时答题（Timeup）→ emit `gate://timeout-reached`。
fn handle_timeout(app: &AppHandle, state: &AppState, round_sec: i64, now_ts: i64) {
    let (pid, target_id, level) = {
        let gate = state.gate.lock().unwrap();
        (
            gate.managed_pid,
            gate.managed_target_id.clone(),
            gate.current_level,
        )
    };
    // 1) 两段式终止目标进程（U8）
    if let Some(pid) = pid {
        let killed = monitor::terminate(pid);
        tracing::info!(event = "timeout_kill", pid, killed, "使用时长到点，终止目标进程");
    }
    // 2) AnswerGate 窗口置顶带回前台
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_focus();
    }
    // 3) 自动进入超时答题（复用 start_quiz 流程，QuizReason=Timeup）
    let level = level.unwrap_or(state.config.quiz.default_level);
    let count = state.config.quiz.question_count as usize;
    let exclude_fill_blank = cfg!(target_os = "android");
    let target_name = target_id
        .as_deref()
        .and_then(|id| state.config.target_apps.iter().find(|t| t.id == id))
        .map(|t| t.name.clone())
        .unwrap_or_default();
    let quiz_result = quiz::make_paper(&state.bank, level, count, exclude_fill_blank).and_then(
        |paper| {
            let mut gate = state.gate.lock().unwrap();
            // 时间继续流逝可能已越过本轮时长——以当前时间重验剩余，仅作日志（到点即超时答题）
            let _ = gate.round_remaining_sec(now_ts, round_sec);
            gate.begin_quiz_reason(level, target_id.clone(), QuizReason::Timeup, now())?;
            gate.set_active_paper(paper.clone());
            gate.save(&state.state_file())?;
            Ok(paper.to_view())
        },
    );
    match quiz_result {
        Ok(_) => {
            let _ = app.emit(
                "gate://timeout-reached",
                serde_json::json!({ "target_name": target_name }),
            );
        }
        Err(e) => {
            // 入题失败兜底（如题库异常）：放弃本轮回 Idle，不放任目标继续使用
            tracing::warn!(error = %e, event = "timeout_kill", result = "quiz_start_failed", "超时后进入答题失败，放弃本轮");
            let mut gate = state.gate.lock().unwrap();
            gate.abandon_round();
            let _ = gate.save(&state.state_file());
            let _ = app.emit("gate://process-exited", serde_json::json!({}));
        }
    }
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

/// 解锁启动目标软件（v2：PASS 后纳管启动并进入使用中计时，应用保持常驻）。
#[tauri::command]
pub fn launch_target(state: State<AppState>, app_handle: AppHandle) -> Result<(), AppError> {
    do_launch_target(&state, &app_handle, false, now())
}

/// 「继续使用」（v2：超时答题通过后续期拉起——Passed{Timeup} → 纳管启动新轮次）。
#[tauri::command]
pub fn extend_and_relaunch(
    state: State<AppState>,
    app_handle: AppHandle,
) -> Result<(), AppError> {
    do_launch_target(&state, &app_handle, false, now())
}

/// 使用中页数据（错误：NotUsing）。
#[tauri::command]
pub fn get_monitor_state(state: State<AppState>) -> Result<MonitorStateView, AppError> {
    do_monitor_view(&state, now())
}

/// 「提前结束本轮」：停止监控 → kill 目标（若存活）→ abandon_round → 回 IDLE。
#[tauri::command]
pub fn stop_monitor(state: State<AppState>, app_handle: AppHandle) -> Result<(), AppError> {
    let pid = {
        let gate = state.gate.lock().unwrap();
        if gate.phase != GatePhase::Using {
            return Err(AppError::NotUsing);
        }
        gate.managed_pid
    };
    state.monitor_stop.store(true, Ordering::SeqCst);
    if let Some(pid) = pid {
        monitor::terminate(pid);
    }
    {
        let mut gate = state.gate.lock().unwrap();
        gate.abandon_round();
        gate.save(&state.state_file())?;
    }
    let _ = app_handle.emit("gate://monitor-stopped", serde_json::json!({}));
    tracing::info!(event = "round_abandoned", reason = "user_stop", "用户提前结束本轮");
    Ok(())
}

/// 窗口关闭二次确认后调用：与 v1 exit 等价（关掉 AnswerGate = 放弃续期能力，可接受；
/// 本轮剩余时长在重启恢复时按 round_started_epoch 续算）。
#[tauri::command]
pub fn confirm_exit(app_handle: AppHandle) -> Result<(), AppError> {
    tracing::warn!(event = "confirm_exit", "用户确认退出 AnswerGate");
    app_handle.exit(0);
    Ok(())
}

/// 下发当前活动试卷视图（v2：超时答题由后端自动开卷，前端经此取题面）。
///
/// 仅 Quiz 态有活动试卷；无试卷（前端刷新保护场景）返回 PaperNotFound。
#[tauri::command]
pub fn get_active_paper(state: State<AppState>) -> Result<PaperView, AppError> {
    let gate = state.gate.lock().unwrap();
    let paper = gate
        .active_paper()
        .ok_or_else(|| AppError::PaperNotFound("无进行中的试卷".to_string()))?;
    Ok(paper.to_view())
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
