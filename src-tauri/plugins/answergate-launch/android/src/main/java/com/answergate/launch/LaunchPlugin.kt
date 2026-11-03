package com.answergate.launch

import android.app.Activity
import android.content.Intent
import android.util.Log
import app.tauri.Plugin
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject

/**
 * AnswerGate 启动插件：按包名跳转白名单 App。
 *
 * 注意（Android 11+ 包可见性限制）：
 * 目标包名必须在应用 AndroidManifest.xml 的 <queries> 中声明，
 * 否则 getLaunchIntentForPackage 返回 null（表现为“未安装”）。
 * 该白名单由 scripts/sync_android_queries.py 从 resources/config.toml 生成。
 */
@TauriPlugin
class LaunchPlugin(private val activity: Activity) : Plugin(activity) {

    companion object {
        private const val TAG = "AnswerGateLaunch"
    }

    @Command
    fun launch(invoke: Invoke) {
        val args = invoke.parseArgs(LaunchArgs::class.java)
        val packageName = args.packageName
        if (packageName.isNullOrBlank()) {
            invoke.reject("缺少 packageName 参数")
            return
        }
        try {
            // 未安装/不可见时返回 null；已安装则返回可启动的入口 Intent
            val intent: Intent? = activity.packageManager.getLaunchIntentForPackage(packageName)
            if (intent == null) {
                invoke.reject(
                    "目标 App 未安装或不可见：$packageName" +
                        "（请检查 AndroidManifest <queries> 白名单是否包含该包名）"
                )
                return
            }
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            activity.startActivity(intent)
            Log.i(TAG, "launched: $packageName")
            val ret = JSObject()
            ret.put("launched", packageName)
            invoke.resolve(ret)
        } catch (e: Exception) {
            // 涵盖 NameNotFoundException 等所有启动期异常
            Log.e(TAG, "launch failed: $packageName", e)
            invoke.reject("启动失败：${e.message}")
        }
    }
}

/** 与 Rust 侧 mobile.rs LaunchArgs 对齐（字段 packageName）。 */
data class LaunchArgs(val packageName: String? = null)
