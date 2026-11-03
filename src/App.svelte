<script lang="ts">
  /** 根组件：按门禁流程 phase 切换页面（状态权威在 Rust，store 仅镜像） */
  import { onMount } from "svelte";
  import { page, bootstrap, bootError } from "./lib/gateStore";
  import LevelSelectPage from "./pages/LevelSelectPage.svelte";
  import TargetSelectPage from "./pages/TargetSelectPage.svelte";
  import QuizPage from "./pages/QuizPage.svelte";
  import ResultPage from "./pages/ResultPage.svelte";
  import CooldownPage from "./pages/CooldownPage.svelte";

  onMount(() => {
    void bootstrap();
  });
</script>

{#if $bootError}
  <main class="boot-error">
    <p>⚠️ 启动失败：{$bootError}</p>
  </main>
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

<style>
  .boot-error {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    font-size: var(--ag-font-body);
    color: var(--ag-danger);
  }
</style>
