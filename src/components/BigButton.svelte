<script lang="ts">
  /**
   * 通用大按钮（UI 规范：高 ≥64px、正文 ≥22px、触屏友好）。
   * kind: primary 主色 / success 通过 / danger 危险 / neutral 次要
   */
  import { createEventDispatcher } from "svelte";

  export let label = "";
  export let kind: "primary" | "success" | "danger" | "neutral" = "primary";
  export let disabled = false;

  const dispatch = createEventDispatcher<{ click: void }>();
</script>

<button
  class={`big-btn ${kind}`}
  {disabled}
  on:click={() => dispatch("click")}
  type="button"
>
  {#if label}
    {label}
  {:else}
    <slot />
  {/if}
</button>

<style>
  .big-btn {
    min-height: 64px;
    width: 100%;
    max-width: 560px;
    padding: 12px 28px;
    font-size: 22px;
    font-family: inherit;
    font-weight: 600;
    color: #fff;
    border: none;
    border-radius: var(--ag-radius);
    cursor: pointer;
    transition: transform 0.08s ease, filter 0.15s ease;
  }

  .big-btn:active {
    transform: scale(0.97);
  }

  .big-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .primary {
    background: var(--ag-primary);
  }

  .primary:hover:not(:disabled) {
    background: var(--ag-primary-dark);
  }

  .success {
    background: var(--ag-success);
  }

  .danger {
    background: var(--ag-danger);
  }

  .neutral {
    background: var(--ag-text-secondary);
  }
</style>
