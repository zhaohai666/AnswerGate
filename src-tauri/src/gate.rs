//! 门禁状态机：IDLE → QUIZ → GRADING → (PASS → Using 纳管计时) | (FAIL → RETRY / COOLDOWN → QUIZ)。
//!
//! v2 增量（轮次制使用时长管控）：
//! - `GatePhase::Using`：PASS 后纳管目标软件计时使用（`round_started_epoch` 持久化，防重启绕过）；
//! - `QuizReason::{Initial, Timeup}`：区分首次答题与超时答题（Timeup 判分通过后走续期拉起）；
//! - Using → Idle 两条非答题路径：进程意外退出（alive=false）、`abandon_round`（用户提前结束/取句柄失败），均不触发答题。
//!
//! 防绕过约定（PRD P0-06 / 共享知识 #3）：
//! - 重试/冷却/轮次状态只在 GateState 维护，持久化到 app_data_dir 下独立状态文件；
//! - 文件名采用非常规隐藏名（架构 U1 默认决策），与 config.toml 分离，删 config 不重置冷却；
//! - 内部时间一律存 Unix epoch 秒（i64）；
//! - 重启恢复：冷却未结束 → 仍处 Cooldown；Quiz/Grading/Passed{Initial} → 回 Idle（保留重试余额）；
//!   Passed{Timeup} → 保留（等待"继续使用"续期）；Using → 保留，由装配层用
//!   `monitor::alive` 与剩余时长重验（见 `commands::revalidate_using_on_startup`）。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::model::{Level, Paper};

/// 状态文件名：非常规隐藏名（架构 U1），内容为 JSON。
pub const STATE_FILE_NAME: &str = ".answergate_gate_state.json";

/// 门禁阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GatePhase {
    Idle,
    Quiz,
    Grading,
    Passed,
    Failed,
    Cooldown,
    /// 答题通过后纳管使用中（v2 新增）。
    Using,
}

/// 答题原因（v2 新增）：区分首次答题与超时后答题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuizReason {
    /// 首次答题（解锁轮次）。
    #[default]
    Initial,
    /// 使用时长到点后的超时答题（判分通过走续期拉起）。
    Timeup,
}

/// 门禁状态（active_paper 仅内存持有，不落盘——答案不持久化）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateState {
    pub phase: GatePhase,
    pub remaining_retry: u32,
    pub max_retry: u32,
    #[serde(default)]
    pub cooldown_until_epoch: Option<i64>,
    #[serde(default)]
    pub selected_target_id: Option<String>,
    #[serde(default)]
    pub current_level: Option<Level>,
    /// 当前答题原因（v2：Initial=首次 / Timeup=超时答题）。
    #[serde(default)]
    pub quiz_reason: QuizReason,
    /// 已用轮次数（v2：本轮 Using 会话内累计，含首次与续期）。
    #[serde(default)]
    pub round_count: u32,
    /// 本轮开始时间（v2：Using 态持久化，重启按剩余时长续算，防杀 AnswerGate 绕计时）。
    #[serde(default)]
    pub round_started_epoch: Option<i64>,
    /// 纳管目标进程 pid（v2：重启后用 monitor::alive 重验）。
    #[serde(default)]
    pub managed_pid: Option<i32>,
    /// 纳管目标 id（v2：重启后据此查 use_minutes 续算）。
    #[serde(default)]
    pub managed_target_id: Option<String>,
    #[serde(skip)]
    pub active_paper: Option<Paper>,
}

impl GateState {
    /// 新建初始状态（Idle，重试余额满额）。
    pub fn new(max_retry: u32) -> Self {
        GateState {
            phase: GatePhase::Idle,
            remaining_retry: max_retry,
            max_retry,
            cooldown_until_epoch: None,
            selected_target_id: None,
            current_level: None,
            quiz_reason: QuizReason::Initial,
            round_count: 0,
            round_started_epoch: None,
            managed_pid: None,
            managed_target_id: None,
            active_paper: None,
        }
    }

    /// 回到 Idle（清空临时上下文；重试余额保留或按调用处重置）。
    fn reset_to_idle(&mut self) {
        self.phase = GatePhase::Idle;
        self.active_paper = None;
        self.selected_target_id = None;
        self.current_level = None;
        self.clear_round_fields();
    }

    /// 清空轮次纳管字段（abandon/离开 Using 共用）。
    fn clear_round_fields(&mut self) {
        self.round_started_epoch = None;
        self.managed_pid = None;
        self.managed_target_id = None;
    }

    /// 冷却剩余秒数；不在冷却或已归零返回 None。
    pub fn cooldown_remaining(&self, now: i64) -> Option<u64> {
        if self.phase != GatePhase::Cooldown {
            return None;
        }
        let until = self.cooldown_until_epoch?;
        if until > now {
            Some((until - now) as u64)
        } else {
            None
        }
    }

    /// 进入答题（QUIZ）：冷却中则拒绝；使用中（Using）不允许直接开新答题。
    pub fn begin_quiz(
        &mut self,
        level: Level,
        target_id: Option<String>,
        now: i64,
    ) -> Result<(), AppError> {
        self.begin_quiz_reason(level, target_id, QuizReason::Initial, now)
    }

    /// 进入答题并标记原因（v2：Timeup 由 monitor 超时路径调用）。
    ///
    /// - 冷却中 → 拒绝（CooldownActive）；
    /// - Using 中仅允许 Timeup（超时答题，离开 Using 并清纳管字段）；
    /// - Timeup 答题不消耗重试余额（语义与 v1 首答一致，余额只在 begin_retry 扣减）。
    pub fn begin_quiz_reason(
        &mut self,
        level: Level,
        target_id: Option<String>,
        reason: QuizReason,
        now: i64,
    ) -> Result<(), AppError> {
        if let Some(remaining_sec) = self.cooldown_remaining(now) {
            return Err(AppError::CooldownActive { remaining_sec });
        }
        if self.phase == GatePhase::Using {
            if reason != QuizReason::Timeup {
                return Err(AppError::InvalidInput(
                    "使用中，不能开始新答题（请先提前结束本轮）".to_string(),
                ));
            }
            // 超时答题：离开 Using，清纳管字段（进程已被 kill）
            self.clear_round_fields();
        }
        self.phase = GatePhase::Quiz;
        self.current_level = Some(level);
        self.selected_target_id = target_id;
        self.quiz_reason = reason;
        Ok(())
    }

    /// 绑定当前试卷（仅内存）。
    pub fn set_active_paper(&mut self, paper: Paper) {
        self.active_paper = Some(paper);
    }

    /// 当前试卷引用。
    pub fn active_paper(&self) -> Option<&Paper> {
        self.active_paper.as_ref()
    }

    /// 提交判分后的状态迁移：QUIZ → GRADING → PASSED / FAILED / COOLDOWN。
    ///
    /// v2：入参增 `reason`；PASS 时落 `quiz_reason=reason`——
    /// Passed{Timeup} 供 command 层走续期拉起分支（phase 仍为 Passed）。
    /// 失败语义（架构约定）：`FAIL 且 remaining_retry==0 时自动进入 COOLDOWN`；
    /// 重试余额在 `begin_retry` 时扣减。max_retry=2 → 首答 + 2 次重试共 3 次机会。
    pub fn record_result(
        &mut self,
        passed: bool,
        reason: QuizReason,
        cool_down_sec: u64,
        now: i64,
    ) -> GatePhase {
        self.phase = GatePhase::Grading;
        self.active_paper = None;
        if passed {
            self.quiz_reason = reason;
            self.phase = GatePhase::Passed;
        } else if self.remaining_retry == 0 {
            self.phase = GatePhase::Cooldown;
            self.cooldown_until_epoch = Some(now + cool_down_sec as i64);
        } else {
            self.phase = GatePhase::Failed;
        }
        self.phase
    }

    /// 「再试一次」：扣减重试余额，QUIZ 保持，重新抽题由上层完成。
    /// quiz_reason 保持（Timeup 轮次的重试仍属超时轮次）。
    pub fn begin_retry(&mut self, now: i64) -> Result<(), AppError> {
        if let Some(remaining_sec) = self.cooldown_remaining(now) {
            return Err(AppError::CooldownActive { remaining_sec });
        }
        if self.remaining_retry == 0 {
            return Err(AppError::NoRetryLeft);
        }
        self.remaining_retry -= 1;
        self.phase = GatePhase::Quiz;
        Ok(())
    }

    /// 冷却到点后复位为 Idle（重试余额恢复满额）。返回是否发生复位。
    pub fn finish_cooldown_if_elapsed(&mut self, now: i64) -> bool {
        if self.phase == GatePhase::Cooldown && self.cooldown_remaining(now).is_none() {
            self.cooldown_until_epoch = None;
            self.reset_to_idle();
            self.remaining_retry = self.max_retry;
            true
        } else {
            false
        }
    }

    // ---------- v2 轮次制（Using） ----------

    /// 进入 Using（首次纳管）：Passed → Using。
    ///
    /// 记 `round_started_epoch=now`、`round_count+1`、纳管 pid/目标；取代 v1 的 exit 路径。
    /// `force`（家长应急解锁）跳过前置相位校验；已在 Using → AlreadyManaged。
    pub fn begin_using(
        &mut self,
        target_id: &str,
        pid: i32,
        now: i64,
        force: bool,
    ) -> Result<(), AppError> {
        if self.phase == GatePhase::Using {
            return Err(AppError::AlreadyManaged);
        }
        if !force && self.phase != GatePhase::Passed {
            return Err(AppError::InvalidInput(
                "未通过答题校验，不能进入使用态".to_string(),
            ));
        }
        self.enter_using(target_id, pid, now);
        Ok(())
    }

    /// 续期（Timeup 判分通过后）：Passed{Timeup} → Using。
    ///
    /// 重置 `round_started_epoch=now`，`round_count+1`。
    pub fn extend_round(&mut self, target_id: &str, pid: i32, now: i64) -> Result<(), AppError> {
        if self.phase == GatePhase::Using {
            return Err(AppError::AlreadyManaged);
        }
        if self.phase != GatePhase::Passed || self.quiz_reason != QuizReason::Timeup {
            return Err(AppError::InvalidInput(
                "仅超时答题通过后可续期".to_string(),
            ));
        }
        self.enter_using(target_id, pid, now);
        Ok(())
    }

    /// Using 进入的公共落盘字段。
    fn enter_using(&mut self, target_id: &str, pid: i32, now: i64) {
        self.phase = GatePhase::Using;
        self.round_count += 1;
        self.round_started_epoch = Some(now);
        self.managed_pid = Some(pid);
        self.managed_target_id = Some(target_id.to_string());
        self.active_paper = None;
    }

    /// 提前结束/句柄丢失/进程意外退出共用：Using|Passed → Idle，清轮次字段，不进 QUIZ。
    pub fn abandon_round(&mut self) {
        if matches!(self.phase, GatePhase::Using | GatePhase::Passed) {
            self.phase = GatePhase::Idle;
            self.active_paper = None;
            self.clear_round_fields();
        }
    }

    /// 本轮剩余秒数；非 Using 或无开始时间返回 None。
    pub fn round_remaining_sec(&self, now: i64, round_sec: i64) -> Option<i64> {
        if self.phase != GatePhase::Using {
            return None;
        }
        let started = self.round_started_epoch?;
        Some(started + round_sec - now)
    }

    /// 持久化到指定路径（JSON）。
    pub fn save(&self, path: &Path) -> Result<(), AppError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                AppError::Storage(format!("创建状态目录失败 {}: {}", parent.display(), e))
            })?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Storage(format!("状态序列化失败：{}", e)))?;
        fs::write(path, json).map_err(|e| {
            AppError::Storage(format!("状态写入失败 {}: {}", path.display(), e))
        })
    }

    /// 读取持久化状态；文件不存在返回 None，损坏返回错误（由 recover 记日志兜底）。
    pub fn load(path: &Path) -> Result<Option<GateState>, AppError> {
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(path).map_err(|e| {
            AppError::Storage(format!("状态文件读取失败 {}: {}", path.display(), e))
        })?;
        let state: GateState = serde_json::from_str(&text)
            .map_err(|e| AppError::Storage(format!("状态文件损坏：{}", e)))?;
        Ok(Some(state))
    }

    /// 启动恢复（防重启绕过）：
    /// - 冷却未结束 → 维持 Cooldown（前端直接进冷却页）；
    /// - 冷却已结束 → 复位 Idle、重试余额满额；
    /// - Quiz/Grading/Passed{Initial} → 回 Idle（保留重试余额，无法续答）；
    /// - Passed{Timeup} → 保留（等待"继续使用"续期拉起）；
    /// - Using → 保留：由装配层用 `monitor::alive` + 剩余时长重验（C10/C13），
    ///   失效则 abandon 回 Idle；
    /// - 文件缺失 → 全新状态；文件损坏 → 记日志并重置（U1 应用层边界）。
    pub fn recover(path: &Path, max_retry: u32, now: i64) -> GateState {
        match GateState::load(path) {
            Ok(Some(mut state)) => {
                state.finish_cooldown_if_elapsed(now);
                if state.phase == GatePhase::Passed
                    && state.quiz_reason == QuizReason::Initial
                    || matches!(state.phase, GatePhase::Quiz | GatePhase::Grading)
                {
                    state.reset_to_idle();
                }
                state
            }
            Ok(None) => GateState::new(max_retry),
            Err(e) => {
                tracing::warn!(error = %e, "门禁状态恢复失败，重置为初始状态");
                GateState::new(max_retry)
            }
        }
    }
}
