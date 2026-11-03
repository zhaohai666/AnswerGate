/**
 * 全局状态 store：仅镜像 Rust 侧状态（状态权威在后端，共享知识 #3）。
 * 页面刷新/重启时通过 getGateState 重建（冷却中直接进冷却页，防重启绕过）。
 * v2：新增 monitoring 页（使用中计时）、超时答题流转、提前结束、关窗确认。
 */
import { get, writable } from "svelte/store";
import { api, errCode, errText } from "./api";
import type {
  Answer,
  GradeResult,
  Level,
  MonitorStateView,
  PaperView,
  RulesSummary,
  TargetAppView,
} from "./types";

export type Page =
  | "level"
  | "target"
  | "quiz"
  | "result"
  | "cooldown"
  | "monitoring";

/** 当前页面（按门禁流程切换） */
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

// ---------- v2 使用中（monitoring） ----------

/** 使用中页数据快照（目标名/轮次数等静态字段） */
export const monitor = writable<MonitorStateView | null>(null);
/** 使用中剩余秒数（gate://using-tick 事件驱动，前端不自算） */
export const monitorRemaining = writable<number>(0);
/** 剩余 ≤60s 警告位（前端变红） */
export const monitorWarning = writable<boolean>(false);
/** 环形倒计时总量（进入监控页时的剩余值，约等于整轮时长） */
export const monitorTotal = writable<number>(0);
/** 超时答题顶部提示条文案（null = 非超时答题） */
export const quizBanner = writable<string | null>(null);
/** 本轮答题是否源自超时（ResultPage 据此走「继续使用」分支） */
export const cameFromTimeout = writable<boolean>(false);
/** 关窗二次确认弹窗开关（gate://close-requested 置位） */
export const showExitConfirm = writable<boolean>(false);
/** 底部提示条（进程退出/提前结束等通知） */
export const exitNotice = writable<string | null>(null);

/** 应用启动引导：拉规则/目标/门禁状态；冷却进冷却页、Using 进监控页（重启续算） */
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
      return;
    }
    if (gs.phase === "using") {
      // 重启续算场景：后端 setup 已恢复监控线程，前端直接进使用中页
      const ok = await enterMonitoring();
      if (ok) return;
    }
    page.set("level");
  } catch (e) {
    bootError.set(errText(e));
  }
}

/** 进入监控页：拉取监控快照并初始化剩余秒数；失败返回 false 回学段页 */
async function enterMonitoring(): Promise<boolean> {
  try {
    const m = await api.getMonitorState();
    applyMonitorState(m);
    page.set("monitoring");
    return true;
  } catch (e) {
    exitNotice.set(errText(e));
    page.set("level");
    return false;
  }
}

/** 应用监控快照（剩余/警告/总量） */
function applyMonitorState(m: MonitorStateView): void {
  monitor.set(m);
  monitorRemaining.set(m.remaining_sec);
  monitorWarning.set(m.warning);
  // 进入/续期监控页即新一轮：环形总量取当前剩余（重启续算场景为部分值，可接受）
  monitorTotal.set(m.remaining_sec);
}

// ---------- 事件处理器（App.svelte 全局订阅，共享知识 #2：Rust 计时权威） ----------

/** gate://using-tick：每秒更新剩余与警告位 */
export function handleUsingTick(remainingSec: number, warning: boolean): void {
  monitorRemaining.set(remainingSec);
  monitorWarning.set(warning);
}

/** gate://timeout-reached：后端已 kill+置顶+自动开超时答题卷 → 取卷进答题页 */
export async function handleTimeoutReached(targetName: string): Promise<void> {
  monitor.set(null);
  cameFromTimeout.set(true);
  quizBanner.set(
    targetName
      ? `使用时间到！${targetName} 已暂停，答题通过后可继续使用`
      : "使用时间到！答题通过后可继续使用",
  );
  try {
    const p = await api.getActivePaper();
    paper.set(p);
    result.set(null);
    page.set("quiz");
  } catch (e) {
    // 取卷失败兜底：回学段页（后端已 abandon，不会放任目标继续使用）
    exitNotice.set(errText(e));
    page.set("level");
  }
}

/** gate://process-exited：纳管进程意外退出（后端已 abandon，不触发答题） */
export function handleProcessExited(): void {
  monitor.set(null);
  if (get(page) === "monitoring") {
    exitNotice.set("目标软件已退出，本轮结束");
    page.set("level");
  }
}

/** gate://monitor-stopped：提前结束成功，回学段页 */
export function handleMonitorStopped(): void {
  monitor.set(null);
  if (get(page) === "monitoring") {
    page.set("level");
  }
}

// ---------- 门禁流程 ----------

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
    cameFromTimeout.set(false);
    quizBanner.set(null);
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
  cameFromTimeout.set(false);
  quizBanner.set(null);
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

/** 解锁启动目标软件（v2：纳管启动成功后进入使用中页） */
export async function launch(): Promise<void> {
  launchError.set(null);
  try {
    await api.launchTarget();
    launched.set(true);
    cameFromTimeout.set(false);
    quizBanner.set(null);
    await enterMonitoring();
  } catch (e) {
    launchError.set(errText(e));
  }
}

/** 「继续使用」（v2：超时答题通过后续期拉起新一轮） */
export async function extendAndRelaunch(): Promise<void> {
  launchError.set(null);
  try {
    await api.extendAndRelaunch();
    launched.set(true);
    cameFromTimeout.set(false);
    quizBanner.set(null);
    await enterMonitoring();
  } catch (e) {
    launchError.set(errText(e));
  }
}

/** 「提前结束本轮」（v2：需 ConfirmDialog 二次确认后调用） */
export async function stopMonitor(): Promise<void> {
  try {
    await api.stopMonitor();
    // gate://monitor-stopped 事件负责页面切换，此处无需处理
  } catch (e) {
    alert(errText(e));
  }
}

/** 关窗二次确认：确认后真正退出（关掉 AnswerGate = 放弃续期能力，可接受） */
export async function confirmExit(): Promise<void> {
  showExitConfirm.set(false);
  try {
    await api.confirmExit();
  } catch (e) {
    exitNotice.set(errText(e));
  }
}

/** 返回学段选择页（清空临时数据） */
export function backToLevel(): void {
  paper.set(null);
  result.set(null);
  launchError.set(null);
  launched.set(false);
  cameFromTimeout.set(false);
  quizBanner.set(null);
  monitor.set(null);
  page.set("level");
}
