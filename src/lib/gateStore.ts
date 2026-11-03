/**
 * 全局状态 store：仅镜像 Rust 侧状态（状态权威在后端，共享知识 #3）。
 * 页面刷新/重启时通过 getGateState 重建（冷却中直接进冷却页，防重启绕过）。
 */
import { get, writable } from "svelte/store";
import { api, errCode, errText } from "./api";
import type {
  Answer,
  GradeResult,
  Level,
  PaperView,
  RulesSummary,
  TargetAppView,
} from "./types";

export type Page = "level" | "target" | "quiz" | "result" | "cooldown";

/** 当前页面（按门禁流程线性切换） */
export const page = writable<Page>("level");
export const rules = writable<RulesSummary | null>(null);
export const targets = writable<TargetAppView[]>([]);
export const selectedLevel = writable<Level | null>(null);
export const selectedTargetId = writable<string | null>(null);
export const paper = writable<PaperView | null>(null);
export const result = writable<GradeResult | null>(null);
/** 冷却剩余秒数（由 Rust 事件驱动更新） */
export const cooldownRemaining = writable<number>(0);
/** launch_target 调用失败信息（null = 无错误） */
export const launchError = writable<string | null>(null);
/** 已成功调用 launch_target */
export const launched = writable<boolean>(false);
/** 启动引导失败信息（get_gate_state 等失败） */
export const bootError = writable<string | null>(null);

/** 应用启动引导：拉规则/目标/门禁状态，冷却中直接进冷却页 */
export async function bootstrap(): Promise<void> {
  try {
    const [r, t, gs] = await Promise.all([
      api.getAppConfig(),
      api.listTargets(),
      api.getGateState(),
    ]);
    rules.set(r);
    targets.set(t);
    if (gs.phase === "cooldown") {
      cooldownRemaining.set(gs.cooldown_remaining_sec);
      page.set("cooldown");
    } else {
      page.set("level");
    }
  } catch (e) {
    bootError.set(errText(e));
  }
}

/** 选择学段：多可用目标 → 目标选择页；否则直接开始（Q2 默认决策） */
export async function chooseLevel(level: Level): Promise<void> {
  selectedLevel.set(level);
  const available = get(targets).filter((t) => t.available);
  if (available.length > 1) {
    selectedTargetId.set(null);
    page.set("target");
    return;
  }
  await beginQuiz(available.length === 1 ? available[0].id : null);
}

/** 目标选择页确认 */
export async function chooseTarget(id: string): Promise<void> {
  await beginQuiz(id);
}

/** 开始答题（含 CooldownActive 自动转冷却页） */
export async function beginQuiz(targetId: string | null): Promise<void> {
  const level = get(selectedLevel);
  if (!level) {
    page.set("level");
    return;
  }
  try {
    selectedTargetId.set(targetId);
    const p = await api.startQuiz(level, targetId);
    paper.set(p);
    page.set("quiz");
  } catch (e) {
    if (errCode(e) === "CooldownActive") {
      await gotoCooldown();
      return;
    }
    alert(errText(e));
  }
}

/** 进入冷却页（剩余秒数以 Rust 侧 get_gate_state 为准） */
export async function gotoCooldown(): Promise<void> {
  try {
    const gs = await api.getGateState();
    cooldownRemaining.set(gs.cooldown_remaining_sec);
  } catch {
    // 冷却页只依赖 tick 事件，此处失败不阻塞
  }
  page.set("cooldown");
}

/** 提交整卷（抛出原始错误供 QuizPage 展示） */
export async function submit(answers: Answer[]): Promise<void> {
  const p = get(paper);
  if (!p) return;
  const r = await api.submitAnswers(p.paper_id, answers);
  result.set(r);
  launchError.set(null);
  launched.set(false);
  page.set("result");
}

/** 「再试一次」：重新抽题进答题页 */
export async function retry(): Promise<void> {
  try {
    const p = await api.retryQuiz();
    paper.set(p);
    result.set(null);
    page.set("quiz");
  } catch (e) {
    if (errCode(e) === "CooldownActive") {
      await gotoCooldown();
      return;
    }
    alert(errText(e));
  }
}

/** 解锁启动目标软件 */
export async function launch(): Promise<void> {
  launchError.set(null);
  try {
    await api.launchTarget();
    launched.set(true);
  } catch (e) {
    launchError.set(errText(e));
  }
}

/** 返回学段选择页（清空临时数据） */
export function backToLevel(): void {
  paper.set(null);
  result.set(null);
  launchError.set(null);
  launched.set(false);
  page.set("level");
}
