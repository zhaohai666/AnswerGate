<script lang="ts">
  /**
   * 通用二次确认弹窗（误关确认 / 提前结束确认复用；v2 常驻化）。
   * 大按钮大字体，儿童可读；confirm/cancel 由父组件处理。
   */
  import { createEventDispatcher } from "svelte";

  export let open = false;
  export let title = "确认操作";
  export let message = "";
  export let confirmLabel = "确定";
  export let cancelLabel = "取消";
  /** confirm 按钮配色：danger 危险（默认）/ primary 常规 */
  export let kind: "danger" | "primary" = "danger";

  const dispatch = createEventDispatcher<{ confirm: void; cancel: void }>();
</script>

{#if open}
  <div class="mask" role="dialog" aria-modal="true" aria-label={title}>
    <div class="dialog">
      <h2>{title}</h2>
      <p class="message">{message}</p>
      <div class="actions">
        <button
          class="btn cancel"
          type="button"
          on:click={() => dispatch("cancel")}
        >
          {cancelLabel}
        </button>
        <button
          class={`btn confirm ${kind}`}
          type="button"
          on:click={() => dispatch("confirm")}
        >
          {confirmLabel}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .mask {
    position: fixed;
    inset: 0;
    background: rgba(31, 36, 48, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .dialog {
    background: var(--ag-surface);
    border-radius: var(--ag-radius);
    padding: 28px;
    width: min(520px, 88vw);
    display: flex;
    flex-direction: column;
    gap: 16px;
    text-align: center;
    box-shadow: 0 12px 40px rgba(31, 36, 48, 0.25);
  }

  h2 {
    font-size: var(--ag-font-title);
  }

  .message {
    font-size: var(--ag-font-body);
    color: var(--ag-text-secondary);
    line-height: 1.5;
  }

  .actions {
    display: flex;
    gap: 14px;
    margin-top: 8px;
  }

  .btn {
    flex: 1;
    min-height: var(--ag-btn-height);
    font-size: 22px;
    font-weight: 600;
    color: #fff;
    border: none;
    border-radius: var(--ag-radius);
    cursor: pointer;
    transition: transform 0.08s ease, filter 0.15s ease;
  }

  .btn:active {
    transform: scale(0.97);
  }

  .cancel {
    background: var(--ag-text-secondary);
  }

  .confirm.danger {
    background: var(--ag-danger);
  }

  .confirm.primary {
    background: var(--ag-primary);
  }
</style>
