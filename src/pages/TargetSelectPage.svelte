<script lang="ts">
  /** 目标软件选择页：多白名单时展示，选择后开始答题（PRD Q2） */
  import { targets, chooseTarget, selectedLevel } from "../lib/gateStore";
  import BigButton from "../components/BigButton.svelte";

  $: available = $targets.filter((t) => t.available);
</script>

<main class="container">
  <h1>想玩哪一个？</h1>
  {#if $selectedLevel}
    <p class="subtitle">选择通过答题后要启动的软件（{$selectedLevel} 学段）</p>
  {/if}

  <div class="list">
    {#each available as t (t.id)}
      <BigButton kind="primary" label={t.name} on:click={() => chooseTarget(t.id)} />
    {/each}
  </div>
</main>

<style>
  .container {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
    padding: 16px;
    gap: 18px;
  }

  h1 {
    font-size: var(--ag-font-title);
  }

  .subtitle {
    font-size: var(--ag-font-body);
    color: var(--ag-text-secondary);
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: 100%;
    align-items: center;
  }
</style>
