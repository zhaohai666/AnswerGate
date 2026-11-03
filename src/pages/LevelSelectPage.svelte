<script lang="ts">
  /** 学段选择页：Logo + 三张大卡片 + 规则摘要（PRD 5.3 ①） */
  import { chooseLevel, rules, bootError } from "../lib/gateStore";
  import type { Level } from "../lib/types";

  const levelIcons: Record<Level, string> = {
    kindergarten: "🧸",
    primary: "🎒",
    junior: "📚",
  };

  const levelDescs: Record<Level, string> = {
    kindergarten: "常识 · 数字 · 颜色形状",
    primary: "语文 · 数学 · 英语 · 科学",
    junior: "语数英 · 物化生 · 史地政",
  };
</script>

<main class="container">
  <div class="logo">🚪</div>
  <h1>AnswerGate 答题门禁</h1>
  <p class="subtitle">答对题目，才能解锁软件哦！</p>

  {#if $bootError}
    <p class="error">启动失败：{$bootError}</p>
  {:else if $rules}
    <div class="cards">
      {#each $rules.levels as entry (entry.level)}
        <button
          class="card"
          type="button"
          on:click={() => chooseLevel(entry.level)}
        >
          <span class="icon">{levelIcons[entry.level]}</span>
          <span class="name">{entry.label}</span>
          <span class="desc">{levelDescs[entry.level]}</span>
        </button>
      {/each}
    </div>
    <p class="summary">
      规则：抽 {$rules.question_count} 题 · 正确率 ≥
      {Math.round($rules.pass_score * 100)}% 解锁 · 最多重试 {$rules.max_retry}
      次 · 失败冷却 {$rules.cool_down_sec} 秒
    </p>
  {:else}
    <p class="loading">加载中…</p>
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

  .logo {
    font-size: 56px;
  }

  h1 {
    font-size: var(--ag-font-title);
  }

  .subtitle {
    font-size: var(--ag-font-body);
    color: var(--ag-text-secondary);
  }

  .cards {
    display: flex;
    flex-direction: row;
    gap: 20px;
    width: 100%;
    justify-content: center;
  }

  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    min-height: 180px;
    min-width: 190px;
    padding: 24px 20px;
    background: var(--ag-surface);
    border: 3px solid transparent;
    border-radius: var(--ag-radius);
    box-shadow: 0 4px 14px rgba(31, 36, 48, 0.08);
    font-family: inherit;
    cursor: pointer;
    transition: border-color 0.15s ease, transform 0.1s ease;
  }

  .card:hover {
    border-color: var(--ag-primary);
  }

  .card:active {
    transform: scale(0.97);
  }

  .icon {
    font-size: 44px;
  }

  .name {
    font-size: 26px;
    font-weight: 700;
    color: var(--ag-text);
  }

  .desc {
    font-size: 16px;
    color: var(--ag-text-secondary);
  }

  .summary {
    font-size: 16px;
    color: var(--ag-text-secondary);
  }

  .error {
    color: var(--ag-danger);
    font-size: var(--ag-font-body);
  }

  .loading {
    font-size: var(--ag-font-body);
    color: var(--ag-text-secondary);
  }

  /* Android 竖屏：卡片纵向排列占满屏宽 */
  @media (orientation: portrait) {
    .cards {
      flex-direction: column;
      align-items: stretch;
    }

    .card {
      min-width: 0;
      min-height: 96px;
      flex-direction: row;
      justify-content: flex-start;
      gap: 20px;
    }
  }
</style>
