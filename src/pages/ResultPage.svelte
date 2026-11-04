<script lang="ts">
  /**
   * 结果页：通过态（v2 分支——Initial：启动倒计时 → 纳管进使用中页；
   * Timeup：「继续使用」续期按钮）；失败态（得分对比 + 剩余重试 + 再试一次；
   * 重试耗尽自动进冷却页）（PRD 5.3 ③）
   */
  import { onDestroy, onMount } from "svelte";
  import {
    result,
    rules,
    launched,
    launchError,
    launch,
    extendAndRelaunch,
    cameFromTimeout,
    cameFromWatch,
    quizReason,
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
  // v3：门槛随答题原因变化——超时续期 / 监控拦截都是「3 题全对」
  $: isStrict = $quizReason === "timeup" || $quizReason === "watch";
  $: passLine = isStrict
    ? Math.round(($rules?.timeup_pass_score ?? 1) * 100)
    : $rules
      ? Math.round($rules.pass_score * 100)
      : 80;
  $: needCooldown = r !== null && !r.passed && r.remaining_retry === 0;
  $: isTimeupPass = r !== null && r.passed && $cameFromTimeout;
  $: isWatchPass = r !== null && r.passed && $cameFromWatch;

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

  // Initial 通过态自动开始启动倒计时；Timeup 通过态等待用户点「继续使用」；
  // 失败态触发抖动；重试耗尽 1.6s 后自动进冷却页
  onMount(() => {
    if (r?.passed) {
      if (!$cameFromTimeout) {
        startCountdown();
      }
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
    {#if isTimeupPass}
      <!-- v2：超时答题通过 → 续期拉起（不退出应用） -->
      <h1>答对啦！</h1>
      <p class="tip">答题通过，可以继续使用目标软件啦！</p>
      {#if $launchError}
        <p class="error">续期失败：{$launchError}</p>
        <div class="actions">
          <BigButton kind="primary" label="重试继续" on:click={extendAndRelaunch} />
          <BigButton kind="neutral" label="返回首页" on:click={backToLevel} />
        </div>
      {:else}
        <div class="actions">
          <BigButton kind="success" label="▶ 继续使用" on:click={extendAndRelaunch} />
        </div>
      {/if}
    {:else if isWatchPass}
      <!-- v3：监控拦截答题通过 → 接管既有进程，弹出 B 站窗口并恢复播放 -->
      <h1>答对啦！</h1>
      <p class="tip">已接管目标软件，视频将自动弹出并继续播放…</p>
      {#if $launchError}
        <p class="error">接管失败：{$launchError}</p>
        <div class="actions">
          <BigButton kind="primary" label="重试接管" on:click={launch} />
          <BigButton kind="neutral" label="返回首页" on:click={backToLevel} />
        </div>
      {:else if $launched}
        <p class="tip">正在进入使用计时…</p>
      {:else}
        <p class="tip">
          即将打开目标软件，<span class="count">{countdown}</span> 秒
        </p>
        <div class="actions">
          <BigButton kind="neutral" label="返回首页" on:click={backToLevel} />
        </div>
      {/if}
    {:else if $launched}
      <p class="tip">已启动，正在进入使用计时…</p>
    {:else if $launchError}
      <p class="error">启动失败：{$launchError}</p>
      <div class="actions">
        <BigButton kind="primary" label="重试启动" on:click={launch} />
        <BigButton kind="neutral" label="返回首页" on:click={backToLevel} />
      </div>
    {:else}
      <h1>校验通过！</h1>
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
