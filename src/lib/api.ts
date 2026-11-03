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

  /** 解锁启动目标软件（成功后 Rust 延迟退出应用） */
  launchTarget(): Promise<void> {
    return invoke<void>("launch_target");
  },

  /** 门禁状态（启动恢复；冷却中返回剩余秒数） */
  getGateState(): Promise<GateStateView> {
    return invoke<GateStateView>("get_gate_state");
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
