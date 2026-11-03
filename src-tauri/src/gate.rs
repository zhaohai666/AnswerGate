//! 门禁状态机：IDLE → QUIZ → GRADING → (PASS → 程序退出) | (FAIL → RETRY / COOLDOWN → QUIZ)。
//!
//! 防绕过约定（PRD P0-06 / 共享知识 #3）：
//! - 重试/冷却状态只在 GateState 维护，持久化到 app_data_dir 下独立状态文件；
//! - 文件名采用非常规隐藏名（架构 U1 默认决策），与 config.toml 分离，删 config 不重置冷却；
//! - 内部时间一律存 Unix epoch 秒（i64）；
//! - 重启恢复：冷却未结束 → 仍处 Cooldown；Quiz/Grading/Passed 无法续答 → 回 Idle（保留重试余额）。

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
            active_paper: None,
        }
    }

    /// 回到 Idle（清空临时上下文；重试余额保留或按调用处重置）。
    fn reset_to_idle(&mut self) {
        self.phase = GatePhase::Idle;
        self.active_paper = None;
        self.selected_target_id = None;
        self.current_level = None;
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

    /// 进入答题（QUIZ）：冷却中则拒绝。
    pub fn begin_quiz(
        &mut self,
        level: Level,
        target_id: Option<String>,
        now: i64,
    ) -> Result<(), AppError> {
        if let Some(remaining_sec) = self.cooldown_remaining(now) {
            return Err(AppError::CooldownActive { remaining_sec });
        }
        self.phase = GatePhase::Quiz;
        self.current_level = Some(level);
        self.selected_target_id = target_id;
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
    /// 失败语义（架构约定）：`FAIL 且 remaining_retry==0 时自动进入 COOLDOWN`；
    /// 重试余额在 `begin_retry` 时扣减。max_retry=2 → 首答 + 2 次重试共 3 次机会。
    pub fn record_result(&mut self, passed: bool, cool_down_sec: u64, now: i64) -> GatePhase {
        self.phase = GatePhase::Grading;
        self.active_paper = None;
        if passed {
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
    /// - Quiz/Grading/Passed → 回 Idle（保留重试余额，无法续答）；
    /// - 文件缺失 → 全新状态；文件损坏 → 记日志并重置（U1 应用层边界）。
    pub fn recover(path: &Path, max_retry: u32, now: i64) -> GateState {
        match GateState::load(path) {
            Ok(Some(mut state)) => {
                state.finish_cooldown_if_elapsed(now);
                if matches!(
                    state.phase,
                    GatePhase::Quiz | GatePhase::Grading | GatePhase::Passed
                ) {
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
