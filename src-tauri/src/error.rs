//! 统一错误类型：所有 command 返回 `Result<T, AppError>`。
//!
//! 序列化约定（前后端共享知识 #1）：
//! `{"code": "CooldownActive", "message": "冷却中，剩余 87 秒", "data": {"remaining_sec": 87}}`
//! 前端按 `code` 分支处理，禁止裸字符串匹配 message。

use serde::ser::SerializeMap;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("非法输入：{0}")]
    InvalidInput(String),

    #[error("题库错误：{0}")]
    QuestionBank(String),

    #[error("配置错误：{0}")]
    Config(String),

    #[error("冷却中，剩余 {remaining_sec} 秒")]
    CooldownActive { remaining_sec: u64 },

    #[error("题量不足：需要 {requested} 题，可用 {available} 题")]
    NotEnoughQuestions { requested: u32, available: u32 },

    #[error("解锁启动失败：{0}")]
    Unlock(String),

    #[error("本地存储错误：{0}")]
    Storage(String),

    #[error("试卷不存在：{0}")]
    PaperNotFound(String),

    #[error("目标软件不存在：{0}")]
    TargetNotFound(String),

    #[error("没有剩余重试次数")]
    NoRetryLeft,

    #[error("家长密码错误")]
    ParentPasswordError,
}

impl AppError {
    /// 错误码（前端分支依据）。
    pub fn code(&self) -> &'static str {
        match self {
            AppError::InvalidInput(_) => "InvalidInput",
            AppError::QuestionBank(_) => "QuestionBank",
            AppError::Config(_) => "Config",
            AppError::CooldownActive { .. } => "CooldownActive",
            AppError::NotEnoughQuestions { .. } => "NotEnoughQuestions",
            AppError::Unlock(_) => "Unlock",
            AppError::Storage(_) => "Storage",
            AppError::PaperNotFound(_) => "PaperNotFound",
            AppError::TargetNotFound(_) => "TargetNotFound",
            AppError::NoRetryLeft => "NoRetryLeft",
            AppError::ParentPasswordError => "ParentPasswordError",
        }
    }

    /// 附加结构化数据（CooldownActive 携带剩余秒数等）。
    pub fn data(&self) -> Option<serde_json::Value> {
        match self {
            AppError::CooldownActive { remaining_sec } => {
                Some(serde_json::json!({ "remaining_sec": remaining_sec }))
            }
            AppError::NotEnoughQuestions {
                requested,
                available,
            } => Some(serde_json::json!({
                "requested": requested,
                "available": available
            })),
            _ => None,
        }
    }
}

/// 手动实现：统一输出 `{code, message, data}` 三字段。
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("code", self.code())?;
        map.serialize_entry("message", &self.to_string())?;
        map.serialize_entry("data", &self.data())?;
        map.end()
    }
}
