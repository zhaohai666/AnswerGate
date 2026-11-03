<script lang="ts">
  /**
   * 使用中页（v2）：目标软件名 + 圆形倒计时（gate://using-tick 事件驱动，
   * 前端不自算）+ 轮次计数 + 「提前结束」（ConfirmDialog 二次确认 → stop_monitor）。
   * 剩余 ≤60s 数字变红；进程意外退出/停止由全局事件切页（App.svelte）。
   */
  import {
    monitor,
    monitorRemaining,
    monitorTotal,
    monitorWarning,
    stopMonitor,
  } from "../lib/gateStore";
  import CountdownRing from "../components/CountdownRing.svelte";
  import BigButton from "../components/BigButton.svelte";
  import ConfirmDialog from "../components/ConfirmDialog.svelte";

  let confirmOpen = false;

  $: targetName = $monitor?.managed_target_name ?? "目标软件";
  $: roundsUsed = $monitor?.rounds_used ?? 0;
</script>

<main class="container">
  <div class="head">
    <span class="pulse" class:warning={$monitorWarning}></span>
    <h1>使用中：{targetName}</h1>
  </div>
  <p class="round">第 <span class="num">{roundsUsed}</span> 轮</p>

  <div class="ring" class:warning={$monitorWarning}>
    <CountdownRing remaining={$monitorRemaining} total={$monitorTotal} />
  </div>

  {#if $monitorWarning}
    <p class="warn-tip">⏰ 时间快用完啦，注意休息哦！</p>
  {:else}
    <p class="tip">时间到会自动关闭 {targetName}，答题通过可继续使用</p>
  {/if}

  <BigButton
    kind="danger"
    label="提前结束本轮"
    on:click={() => (confirmOpen = true)}
  />
</main>

<ConfirmDialog
  open={confirmOpen}
  title="提前结束？"
  message={`现在结束将关闭 ${targetName}，下次使用需要重新答题哦。`}
  confirmLabel="结束本轮"
  cancelLabel="继续使用"
  on:confirm={() => {
    confirmOpen = false;
    void stopMonitor();
  }}
  on:cancel={() => (confirmOpen = false)}
/>

<style>
  .container {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
    padding: 16px;
    gap: 12px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  h1 {
    font-size: var(--ag-font-title);
  }

  /* 呼吸圆点：绿色使用中 / 红色警告 */
  .pulse {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--ag-success);
    animation: breathe 2s ease-in-out infinite;
  }

  .pulse.warning {
    background: var(--ag-danger);
  }

  @keyframes breathe {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }

  .round {
    font-size: var(--ag-font-body);
    color: var(--ag-text-secondary);
  }

  .num {
    font-size: 24px;
    font-weight: 700;
    color: var(--ag-primary);
  }

  /* 警告态：倒计时数字变红（P1-08） */
  .ring :global(.number) {
    transition: color 0.3s ease;
  }

  .ring.warning :global(.number) {
    color: var(--ag-danger);
  }

  .tip,
  .warn-tip {
    font-size: var(--ag-font-body);
    color: var(--ag-text-secondary);
  }

  .warn-tip {
    color: var(--ag-danger);
    font-weight: 600;
  }
</style>
