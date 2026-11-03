//! answergate-launch：自研 Tauri 2 Mobile Plugin（P0-05 Android 分支）。
//!
//! - Android：桥接 Kotlin `LaunchPlugin`（Intent + setPackage 跳转）
//! - 桌面端：桌面解锁由 `unlock.rs` 原生实现，本插件返回不支持错误

use tauri::{
    plugin::{Builder, TauriPlugin},
    AppHandle, Runtime,
};

mod commands;

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

#[cfg(desktop)]
use desktop::launch as platform_launch;
#[cfg(mobile)]
use mobile::launch as platform_launch;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Anyhow(String),
    #[cfg(mobile)]
    #[error("移动插件调用失败：{0}")]
    PluginInvoke(String),
}

impl serde::Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// 注册插件（主应用 lib.rs 中 `.plugin(answergate_launch::init())`）。
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("answergate-launch")
        .invoke_handler(tauri::generate_handler![commands::launch])
        .setup(|app, api| {
            #[cfg(mobile)]
            api.register_android_plugin("com.answergate.launch", "LaunchPlugin")?;
            #[cfg(desktop)]
            desktop::init(app)?;
            #[cfg(not(mobile))]
            let _ = api;
            Ok(())
        })
        .build()
}

/// 启动指定包名的 App（Android 有效；桌面端返回不支持错误）。
pub fn launch<R: Runtime>(app: &AppHandle<R>, package: String) -> Result<String> {
    platform_launch(app, package)
}
