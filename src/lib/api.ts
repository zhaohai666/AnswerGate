/**
 * 前端 ↔ 后端唯一通道：Tauri invoke/listen 的类型化封装。
 * command 命名 snake_case 与 Rust 一致；invoke 参数用 camelCase（Tauri 自动映射）。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Answer,
  GateStateView,
  GradeResult,
  Level,
  MonitorStateView,
  PaperView,
  RulesSummary,
  TargetAppView,
} from "./types";

export const api = {
  /** 规则摘要（题量/及格线/重试/冷却 + 学段列表） */
  getAppConfig(): Promise<RulesSummary> {
    return invoke<RulesSummary>("get_app_config");
  },

  /** 白名单目标软件列表 */
  listTargets(): Promise<TargetAppView[]> {
    return invoke<TargetAppView[]>("list_targets");
  },

  /** 开始答题（targetId 为 null 时由 Rust 自动选择/无需目标） */
  startQuiz(level: Level, targetId: string | null): Promise<PaperView> {
    return invoke<PaperView>("start_quiz", {
      level,
      targetId: targetId ?? null,
    });
  },

  /** 失败后重试（重新抽题） */
  retryQuiz(): Promise<PaperView> {
    return invoke<PaperView>("retry_quiz");
  },

  /** 提交整卷，Rust 权威判分 */
  submitAnswers(paperId: string, answers: Answer[]): Promise<GradeResult> {
    return invoke<GradeResult>("submit_answers", {
      paperId,
      answers,
    });
  },

  /** 解锁启动目标软件（v2：纳管启动并进入使用中计时，应用保持常驻） */
  launchTarget(): Promise<void> {
    return invoke<void>("launch_target");
  },

  /** 「继续使用」（v2：超时答题通过后续期拉起，开启新一轮计时） */
  extendAndRelaunch(): Promise<void> {
    return invoke<void>("extend_and_relaunch");
  },

  /** 使用中页数据（仅 Using 态可调，否则 NotUsing） */
  getMonitorState(): Promise<MonitorStateView> {
    return invoke<MonitorStateView>("get_monitor_state");
  },

  /** 「提前结束本轮」：停止监控 → kill 目标 → 回学段页（gate://monitor-stopped 通知） */
  stopMonitor(): Promise<void> {
    return invoke<void>("stop_monitor");
  },

  /** 关窗二次确认后真正退出应用 */
  confirmExit(): Promise<void> {
    return invoke<void>("confirm_exit");
  },

  /** 下发当前活动试卷（v2：超时答题由后端自动开卷，前端经此取题面） */
  getActivePaper(): Promise<PaperView> {
    return invoke<PaperView>("get_active_paper");
  },

  /** 门禁状态（启动恢复；冷却中返回剩余秒数） */
  getGateState(): Promise<GateStateView> {
    return invoke<GateStateView>("get_gate_state");
  },

  /** 前端诊断日志上报（全局错误兜底，落 answer_gate.log） */
  frontendLog(level: "error" | "warn" | "info", message: string): Promise<void> {
    return invoke<void>("frontend_log", { level, message }).catch(() => {});
  },

  /** 家长密码应急解锁 */
  parentUnlock(password: string): Promise<void> {
    return invoke<void>("parent_unlock", { password });
  },

  /** 订阅冷却倒计时（Rust 每秒推送，前端不自算） */
  async onCooldownTick(cb: (remaining: number) => void): Promise<UnlistenFn> {
    return listen<{ remaining: number }>("gate://cooldown-tick", (event) => {
      cb(event.payload.remaining);
    });
  },

  /** 订阅冷却结束 */
  async onCooldownFinished(cb: () => void): Promise<UnlistenFn> {
    return listen("gate://cooldown-finished", () => {
      cb();
    });
  },

  /** 订阅使用中倒计时（Rust 每秒推送，前端不自算；v2） */
  async onUsingTick(
    cb: (remainingSec: number, warning: boolean) => void,
  ): Promise<UnlistenFn> {
    return listen<{ remaining_sec: number; warning: boolean }>(
      "gate://using-tick",
      (event) => {
        cb(event.payload.remaining_sec, event.payload.warning);
      },
    );
  },

  /** 订阅使用时长到点（后端已 kill 目标 + 置顶 + 自动开超时答题卷；v2） */
  async onTimeoutReached(
    cb: (targetName: string) => void,
  ): Promise<UnlistenFn> {
    return listen<{ target_name: string }>("gate://timeout-reached", (event) => {
      cb(event.payload.target_name);
    });
  },

  /** 订阅纳管进程意外退出（后端已 abandon 回 Idle，不触发答题；v2） */
  async onProcessExited(cb: () => void): Promise<UnlistenFn> {
    return listen("gate://process-exited", () => {
      cb();
    });
  },

  /** 订阅监控停止（stop_monitor 成功后通知，回学段页；v2） */
  async onMonitorStopped(cb: () => void): Promise<UnlistenFn> {
    return listen("gate://monitor-stopped", () => {
      cb();
    });
  },

  /** 订阅关窗拦截事件（后端已 prevent_close，前端弹二次确认；v2） */
  async onCloseRequested(cb: () => void): Promise<UnlistenFn> {
    return listen("gate://close-requested", () => {
      cb();
    });
  },

  /** 订阅纳管启动成功（v2.1：后端兜底自动启动时前端定时器可能被节流，靠此事件进监控页） */
  async onRoundStarted(cb: (targetName: string) => void): Promise<UnlistenFn> {
    return listen<{ target_name: string }>("gate://round-started", (event) => {
      cb(event.payload.target_name);
    });
  },

  /** 订阅后端兜底自动启动失败（v2.1 诊断用） */
  async onAutoLaunchFailed(cb: (message: string) => void): Promise<UnlistenFn> {
    return listen<{ message: string }>("gate://auto-launch-failed", (event) => {
      cb(event.payload.message);
    });
  },
};

/** 将 invoke 抛出的 AppError({code,message,data}) 转为可展示文本 */
export function errText(e: unknown): string {
  if (typeof e === "object" && e !== null && "code" in e) {
    const ae = e as { code: string; message?: string };
    return ae.message && ae.message.length > 0 ? ae.message : ae.code;
  }
  return String(e);
}

/** 取 AppError 的 code（无 code 返回 null） */
export function errCode(e: unknown): string | null {
  if (typeof e === "object" && e !== null && "code" in e) {
    return (e as { code: string }).code;
  }
  return null;
}
