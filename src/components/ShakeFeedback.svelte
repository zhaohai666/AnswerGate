<script lang="ts">
  /**
   * 答错抖动反馈容器：trigger 每自增 1 触发一次抖动动效。
   * 用法：<ShakeFeedback trigger={n}>内容</ShakeFeedback>
   */
  import { onDestroy } from "svelte";

  export let trigger = 0;
  export let duration = 500;

  let shaking = false;
  let lastTrigger = trigger;
  let timer: ReturnType<typeof setTimeout> | undefined;

  $: if (trigger !== lastTrigger) {
    lastTrigger = trigger;
    shaking = true;
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => (shaking = false), duration);
  }

  onDestroy(() => {
    if (timer) clearTimeout(timer);
  });
</script>

<div class:shake={shaking}>
  <slot />
</div>

<style>
  @keyframes ag-shake {
    0%,
    100% {
      transform: translateX(0);
    }
    15% {
      transform: translateX(-12px);
    }
    30% {
      transform: translateX(10px);
    }
    45% {
      transform: translateX(-8px);
    }
    60% {
      transform: translateX(6px);
    }
    75% {
      transform: translateX(-4px);
    }
    90% {
      transform: translateX(2px);
    }
  }

  .shake {
    animation: ag-shake 0.5s ease-in-out;
  }
</style>
