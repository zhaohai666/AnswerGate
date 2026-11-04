<script lang="ts">
  /** 根组件：按门禁流程 phase 切换页面（状态权威在 Rust，store 仅镜像）
   *  v2：monitoring 页路由 + 全局事件订阅 + 关窗二次确认（常驻化） */
  import { onDestroy, onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { api } from "./lib/api";
  import {
    page,
    bootstrap,
    bootError,
    showExitConfirm,
    exitNotice,
    confirmExit,
    handleUsingTick,
    handleTimeoutReached,
    handleProcessExited,
    handleMonitorStopped,
    handleRoundStarted,
    handleWatchIntercepted,
    handleWatchAbandoned,
    permissionWarning,
    gotoCooldown,
  } from "./lib/gateStore";
  import LevelSelectPage from "./pages/LevelSelectPage.svelte";
  import TargetSelectPage from "./pages/TargetSelectPage.svelte";
  import QuizPage from "./pages/QuizPage.svelte";
  import ResultPage from "./pages/ResultPage.svelte";
  import CooldownPage from "./pages/CooldownPage.svelte";
  import MonitoringPage from "./pages/MonitoringPage.svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";

  let unlisteners: UnlistenFn[] = [];
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(async () => {
    void bootstrap();
    try {
      unlisteners = await Promise.all([
        api.onUsingTick((remaining, warning) => handleUsingTick(remaining, warning)),
        api.onTimeoutReached((name) => void handleTimeoutReached(name)),
        api.onProcessExited(() => handleProcessExited()),
        api.onMonitorStopped(() => handleMonitorStopped()),
        api.onCloseRequested(() => showExitConfirm.set(true)),
      ]);
    } catch (e) {
      console.error("事件订阅失败", e);
      void api.frontendLog("error", `事件订阅失败: ${String(e)}`);
    }
    // v2.1 附加通道：后端兜底自动启动 / 冷却结束（各自独立订阅，失败不相互影响）
    try {
      unlisteners.push(await api.onRoundStarted((name) => void handleRoundStarted(name)));
      unlisteners.push(
        await api.onAutoLaunchFailed((msg) => exitNotice.set(`自动启动失败：${msg}`)),
      );
      unlisteners.push(await api.onCooldownFinished(() => void gotoCooldown()));
      // v3 独立监控拦截通道：检测到绕过门禁启动的目标 → 抢答
      unlisteners.push(
        await api.onWatchIntercepted((name) => void handleWatchIntercepted(name)),
      );
      unlisteners.push(await api.onWatchAbandoned(() => handleWatchAbandoned()));
      unlisteners.push(
        await api.onPermissionRequired((msg) => permissionWarning.set(msg)),
      );
    } catch (e) {
      void api.frontendLog("warn", `附加事件订阅失败: ${String(e)}`);
    }
  });

  onDestroy(() => {
    unlisteners.forEach((u) => u());
    unlisteners = [];
    if (noticeTimer) clearTimeout(noticeTimer);
  });

  // 底部提示条 3s 自动消失
  $: if ($exitNotice) {
    if (noticeTimer) clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => exitNotice.set(null), 3000);
  }
</script>

{#if $permissionWarning}
  <div class="permission-bar" role="alert">⚠️ {$permissionWarning}</div>
{/if}

{#if $bootError}
  <main class="boot-error">
    <p>⚠️ 启动失败：{$bootError}</p>
  </main>
{:else if $page === "monitoring"}
  <MonitoringPage />
{:else if $page === "level"}
  <LevelSelectPage />
{:else if $page === "target"}
  <TargetSelectPage />
{:else if $page === "quiz"}
  <QuizPage />
{:else if $page === "result"}
  <ResultPage />
{:else if $page === "cooldown"}
  <CooldownPage />
{/if}

{#if $exitNotice}
  <div class="notice" role="status">{$exitNotice}</div>
{/if}

<!-- 关窗二次确认（后端已 prevent_close，确认后才真正退出） -->
<ConfirmDialog
  open={$showExitConfirm}
  title="要退出 AnswerGate 吗？"
  message="退出后本轮使用剩余时间仍会被记录，下次启动继续计时。确定退出吗？"
  confirmLabel="退出"
  cancelLabel="留在这里"
  on:confirm={() => void confirmExit()}
  on:cancel={() => showExitConfirm.set(false)}
/>

<style>
  .boot-error {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    font-size: var(--ag-font-body);
    color: var(--ag-danger);
  }

  /* 系统权限告警（v3：辅助功能未授权时常驻顶部，不能自动消失） */
  .permission-bar {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    z-index: 100;
    background: var(--ag-warning);
    color: #fff;
    font-size: 14px;
    font-weight: 600;
    line-height: 1.5;
    text-align: center;
    padding: 8px 16px;
    box-shadow: 0 2px 8px rgba(31, 36, 48, 0.18);
  }

  /* 底部提示条（进程退出/提前结束通知） */
  .notice {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--ag-text);
    color: #fff;
    font-size: var(--ag-font-body);
    padding: 12px 24px;
    border-radius: var(--ag-radius);
    box-shadow: 0 6px 20px rgba(31, 36, 48, 0.3);
    z-index: 90;
  }
</style>
