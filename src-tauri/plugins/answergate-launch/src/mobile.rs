//! Android 桥接：经 AppHandle::run_mobile_plugin 调用 Kotlin `LaunchPlugin.launch`。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

use crate::Result;

#[derive(Debug, Serialize, Deserialize)]
struct LaunchArgs {
    #[serde(rename = "packageName")]
    package_name: String,
}

pub fn launch<R: Runtime>(app: &AppHandle<R>, package: String) -> Result<String> {
    app.run_mobile_plugin(
        "launch",
        LaunchArgs {
            package_name: package,
        },
    )
    .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
}
