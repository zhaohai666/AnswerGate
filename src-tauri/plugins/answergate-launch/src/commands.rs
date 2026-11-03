//! 前端可直接 invoke 的 command：`plugin:answergate-launch|launch`。

use tauri::{AppHandle, Runtime};

#[tauri::command]
pub(crate) async fn launch<R: Runtime>(
    app: AppHandle<R>,
    package: String,
) -> Result<String, crate::Error> {
    crate::launch(&app, package)
}
