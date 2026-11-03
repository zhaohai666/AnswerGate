<script lang="ts">
  /**
   * 冷却页：圆形倒计时完全由 Rust gate://cooldown-tick 事件驱动（前端不自算），
   * 归零收 gate://cooldown-finished 自动回学段选择页（PRD 5.3 ④）
   */
  import { onMount, onDestroy } from "svelte";
  import { api } from "../lib/api";
  import { cooldownRemaining, backToLevel } from "../lib/gateStore";
  import CountdownRing from "../components/CountdownRing.svelte";

  let unlisteners: Array<() => void> = [];
  /** 环形总量：取本次会话见过的最大剩余值 */
  let total = 120;

  $: if ($cooldownRemaining > total) total = $cooldownRemaining;

  onMount(async () => {
    unlisteners.push(await api.onCooldownTick((r) => cooldownRemaining.set(r)));
    unlisteners.push(
      await api.onCooldownFinished(() => {
        backToLevel();
      })
    );
  });

  onDestroy(() => {
    unlisteners.forEach((u) => u());
    unlisteners = [];
  });
</script>

<main class="container">
  <div class="face">⏳</div>
  <h1>休息一下</h1>
  <CountdownRing remaining={$cooldownRemaining} {total} />
  <p class="tip">稍后再来答题吧～</p>
</main>

<style>
  .container {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
    gap: 14px;
  }

  .face {
    font-size: 56px;
  }

  h1 {
    font-size: var(--ag-font-title);
  }

  .tip {
    font-size: var(--ag-font-body);
    color: var(--ag-text-secondary);
  }
</style>
