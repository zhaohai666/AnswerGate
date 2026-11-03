<script lang="ts">
  /** 答题页顶部进度条：「第 x / N 题」+ 进度条 */
  export let current = 1;
  export let total = 1;
</script>

<header class="header">
  <div class="text">第 {Math.min(current, total)} / {total} 题</div>
  <div class="bar" role="progressbar" aria-valuenow={current} aria-valuemax={total}>
    <div class="fill" style={`width: ${total > 0 ? (current / total) * 100 : 0}%`}></div>
  </div>
</header>

<style>
  .header {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 20px 4px;
  }

  .text {
    font-size: 20px;
    font-weight: 600;
    color: var(--ag-text);
  }

  .bar {
    height: 12px;
    border-radius: 6px;
    background: #dfe5ef;
    overflow: hidden;
  }

  .fill {
    height: 100%;
    border-radius: 6px;
    background: var(--ag-primary);
    transition: width 0.25s ease;
  }
</style>
