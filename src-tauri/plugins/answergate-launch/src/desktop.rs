//! 桌面端占位实现：桌面解锁由 unlock.rs 原生负责，插件在此平台无能力。

use tauri::{AppHandle, Runtime};

use crate::Result;

pub fn init<R: Runtime>(_app: &AppHandle<R>) -> Result<()> {
    Ok(())
}

pub fn launch<R: Runtime>(_app: &AppHandle<R>, _package: String) -> Result<String> {
    Err(crate::Error::Anyhow(
        "answergate-launch 插件仅用于 Android；桌面端由 unlock.rs 原生启动".to_string(),
    ))
}
