<script lang="ts">
  /** 答题页：单选/判断/填空三种作答区，「上一题」可改答案，整卷一次性提交（PRD 5.3 ②）
   *  v2：超时答题在顶部显示提示条（quizBanner 由 gate://timeout-reached 置位） */
  import { paper, submit, backToLevel, quizBanner } from "../lib/gateStore";
  import type { Answer, QuizItem } from "../lib/types";
  import ProgressHeader from "../components/ProgressHeader.svelte";
  import BigButton from "../components/BigButton.svelte";

  let index = 0;
  let answers: Record<string, string> = {};
  let submitting = false;

  $: items = ($paper?.items ?? []) as QuizItem[];
  $: total = items.length;
  $: current = items[index] as QuizItem | undefined;
  $: given = current ? answers[current.id] ?? "" : "";

  function choose(value: string): void {
    if (!current) return;
    answers = { ...answers, [current.id]: value };
  }

  function prev(): void {
    if (index > 0) index -= 1;
  }

  function next(): void {
    if (index < total - 1) index += 1;
  }

  async function handleSubmit(): Promise<void> {
    if (submitting || !$paper) return;
    submitting = true;
    try {
      const list: Answer[] = items.map((i) => ({
        question_id: i.id,
        value: answers[i.id] ?? "",
      }));
      await submit(list);
    } catch (e) {
      alert(String(e));
    } finally {
      submitting = false;
    }
  }

  // 无试卷时防误入（刷新保护）
  $: if (!$paper) backToLevel();
</script>

{#if $paper}
  <main class="container">
    {#if $quizBanner}
      <div class="timeup-banner" role="alert">⏰ {$quizBanner}</div>
    {/if}
    <ProgressHeader current={index + 1} {total} />

    {#if current}
      <section class="question">
        <h2>{current.title}</h2>

        {#if current.qtype === "single_choice"}
          <div class="options">
            {#each current.options as opt (opt)}
              <button
                class="option"
                class:selected={given === opt}
                type="button"
                on:click={() => choose(opt)}
              >
                {opt}
              </button>
            {/each}
          </div>
        {:else if current.qtype === "true_false"}
          <div class="options two">
            <button
              class="option tf"
              class:selected={given === "对"}
              type="button"
              on:click={() => choose("对")}
            >
              对 ✓
            </button>
            <button
              class="option tf"
              class:selected={given === "错"}
              type="button"
              on:click={() => choose("错")}
            >
              错 ✗
            </button>
          </div>
        {:else}
          <div class="blank">
            <input
              class="blank-input"
              type="text"
              placeholder="在这里输入答案"
              value={given}
              on:input={(e) => choose(e.currentTarget.value)}
              on:keydown={(e) => {
                if (e.key === "Enter") {
                  index < total - 1 ? next() : handleSubmit();
                }
              }}
            />
          </div>
        {/if}
      </section>
    {/if}

    <footer class="nav">
      <BigButton kind="neutral" label="← 上一题" disabled={index === 0} on:click={prev} />
      {#if index < total - 1}
        <BigButton kind="primary" label="下一题 →" on:click={next} />
      {:else}
        <BigButton
          kind="success"
          label={submitting ? "判分中…" : "交卷！"}
          disabled={submitting}
          on:click={handleSubmit}
        />
      {/if}
    </footer>
  </main>
{/if}

<style>
  .container {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  /* 超时答题提示条（v2） */
  .timeup-banner {
    background: var(--ag-warning);
    color: #fff;
    font-size: var(--ag-font-body);
    font-weight: 600;
    text-align: center;
    padding: 10px 16px;
    flex-shrink: 0;
  }

  .question {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 26px;
    padding: 12px 20px;
    overflow: auto;
  }

  h2 {
    font-size: 28px;
    text-align: center;
    line-height: 1.5;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: 100%;
    max-width: 560px;
  }

  .options.two {
    flex-direction: row;
  }

  .option {
    min-height: 64px;
    padding: 12px 24px;
    font-size: 22px;
    font-family: inherit;
    color: var(--ag-text);
    background: var(--ag-surface);
    border: 3px solid #dfe5ef;
    border-radius: var(--ag-radius);
    cursor: pointer;
    transition: border-color 0.12s ease, background 0.12s ease;
  }

  .option.tf {
    font-size: 28px;
    font-weight: 700;
  }

  .option.selected {
    border-color: var(--ag-primary);
    background: #e8f0fe;
  }

  .blank {
    width: 100%;
    max-width: 560px;
  }

  .blank-input {
    width: 100%;
    min-height: 64px;
    padding: 12px 20px;
    font-size: 24px;
    font-family: inherit;
    border: 3px solid #dfe5ef;
    border-radius: var(--ag-radius);
    outline: none;
  }

  .blank-input:focus {
    border-color: var(--ag-primary);
  }

  .nav {
    display: flex;
    gap: 16px;
    justify-content: center;
    padding: 12px 20px 20px;
  }

  .nav > :global(button) {
    max-width: 260px;
  }
</style>
