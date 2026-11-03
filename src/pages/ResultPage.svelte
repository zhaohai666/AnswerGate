<script lang="ts">
  /**
   * 结果页：通过态（✅ + 启动倒计时 + 失败可重试启动）；失败态（得分对比 +
   * 剩余重试 + 再试一次；重试耗尽自动进冷却页）（PRD 5.3 ③）
   */
  import { onDestroy, onMount } from "svelte";
  import {
    result,
    rules,
    launched,
    launchError,
    launch,
    retry,
    gotoCooldown,
    backToLevel,
  } from "../lib/gateStore";
  import BigButton from "../components/BigButton.svelte";
  import ShakeFeedback from "../components/ShakeFeedback.svelte";

  let countdown = 3;
  let timer: ReturnType<typeof setInterval> | undefined;
  let shakeTrigger = 0;

  $: r = $result;
  $: passLine = $rules ? Math.round($rules.pass_score * 100) : 80;
  $: needCooldown = r !== null && !r.passed && r.remaining_retry === 0;

  function startCountdown(): void {
    countdown = 3;
    if (timer) clearInterval(timer);
    timer = setInterval(() => {
      countdown -= 1;
      if (countdown <= 0 && timer) {
        clearInterval(timer);
        timer = undefined;
        void launch();
      }
    }, 1000);
  }

  // 通过态自动开始启动倒计时；失败态触发抖动；重试耗尽 1.6s 后自动进冷却页
  onMount(() => {
    if (r?.passed) {
      startCountdown();
    } else if (r) {
      shakeTrigger += 1;
      if (r.remaining_retry === 0) {
        setTimeout(() => void gotoCooldown(), 1600);
      }
    }
  });

  onDestroy(() => {
    if (timer) clearInterval(timer);
  });
</script>

<main class="container">
  {#if r?.passed}
    <div class="face">✅</div>
    <h1>校验通过！</h1>
    {#if $launched}
      <p class="tip">已启动，本程序即将关闭…</p>
    {:else if $launchError}
      <p class="error">启动失败：{$launchError}</p>
      <div class="actions">
        <BigButton kind="primary" label="重试启动" on:click={launch} />
        <BigButton kind="neutral" label="返回首页" on:click={backToLevel} />
      </div>
    {:else}
      <p class="tip">
        即将启动目标软件，<span class="count">{countdown}</span> 秒
      </p>
      <div class="actions">
        <BigButton kind="neutral" label="返回首页" on:click={backToLevel} />
      </div>
    {/if}
  {:else if r}
    <ShakeFeedback trigger={shakeTrigger}>
      <div class="face">❌</div>
    </ShakeFeedback>
    <h1>还差一点点！</h1>
    <p class="score">
      本次答对 <span class="num">{r.correct}</span> / {r.total} 题（及格线
      {passLine}%）
    </p>
    {#if needCooldown}
      <p class="tip">重试次数用完了，休息一下吧…</p>
    {:else}
      <p class="tip">还可以再试 {r.remaining_retry} 次</p>
      <div class="actions">
        <BigButton kind="primary" label="再试一次" on:click={retry} />
        <BigButton kind="neutral" label="返回首页" on:click={backToLevel} />
      </div>
    {/if}
  {:else}
    <p class="tip">加载中…</p>
  {/if}
</main>

<style>
  .container {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
    padding: 16px;
    gap: 14px;
  }

  .face {
    font-size: 72px;
  }

  h1 {
    font-size: 34px;
  }

  .tip {
    font-size: var(--ag-font-body);
    color: var(--ag-text-secondary);
  }

  .count {
    font-size: 26px;
    font-weight: 700;
    color: var(--ag-primary);
  }

  .score {
    font-size: 24px;
  }

  .num {
    font-size: 30px;
    font-weight: 700;
    color: var(--ag-primary);
  }

  .error {
    font-size: var(--ag-font-body);
    color: var(--ag-danger);
  }

  .actions {
    display: flex;
    gap: 16px;
  }

  .actions > :global(button) {
    max-width: 240px;
  }
</style>
