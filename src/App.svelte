<script lang="ts">
  // T01 占位根组件：T04 将替换为按 gatePhase 切换的四页面路由
  const appName: string = "AnswerGate 答题门禁";
  const scaffoldStage: string = "T01 项目基础设施已就绪";
</script>

<main class="container">
  <h1>🚪 {appName}</h1>
  <p class="stage">{scaffoldStage}</p>
</main>

<style>
  .container {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
    gap: 16px;
  }

  h1 {
    font-size: 32px;
  }

  .stage {
    font-size: 20px;
    color: var(--ag-text-secondary);
  }
</style>
