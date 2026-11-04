/**
 * 与 Rust 序列化结构严格对齐的 TS 类型（唯一契约来源：src-tauri/src/*）。
 * 注意：AppError 统一序列化为 {code, message, data}。
 */

export type Level = "kindergarten" | "primary" | "junior";

export type QuestionType = "single_choice" | "true_false" | "fill_blank";

export type GatePhase =
  | "idle"
  | "quiz"
  | "grading"
  | "passed"
  | "failed"
  | "cooldown"
  | "using";

/**
 * 答题原因：
 * - initial：首次解锁（走 quiz.question_count / quiz.pass_score）
 * - timeup：使用时长到点的续期答题（走 timeup_question_count / timeup_pass_score，默认 3 题全对）
 * - watch：独立监控拦截（孩子绕过门禁自己打开目标软件）——同样走严格档
 */
export type QuizReason = "initial" | "timeup" | "watch";

/** 下发前端的题目视图（无 answer/hint，防作弊约定） */
export interface QuizItem {
  id: string;
  qtype: QuestionType;
  title: string;
  options: string[];
}

/** 下发前端的试卷视图 */
export interface PaperView {
  paper_id: string;
  level: Level;
  items: QuizItem[];
}

/** 前端提交的单题作答 */
export interface Answer {
  question_id: string;
  value: string;
}

/** 单题对错明细（不含正确答案） */
export interface AnswerOutcome {
  question_id: string;
  correct: boolean;
}

/** 判分结果 */
export interface GradeResult {
  paper_id: string;
  total: number;
  correct: number;
  score: number;
  passed: boolean;
  remaining_retry: number;
  details: AnswerOutcome[];
}

export interface LevelEntry {
  level: Level;
  label: string;
}

/** 规则摘要（get_app_config） */
export interface RulesSummary {
  question_count: number;
  pass_score: number;
  max_retry: number;
  cool_down_sec: number;
  default_level: Level;
  levels: LevelEntry[];
  /** 超时续期/监控拦截的题量（v3，默认 3） */
  timeup_question_count: number;
  /** 超时续期/监控拦截的及格线（v3，默认 1.0 = 必须全对） */
  timeup_pass_score: number;
}

/** 目标软件视图（list_targets） */
export interface TargetAppView {
  id: string;
  name: string;
  available: boolean;
}

/** 门禁状态视图（get_gate_state） */
export interface GateStateView {
  phase: GatePhase;
  remaining_retry: number;
  cooldown_remaining_sec: number;
  selected_target_name: string | null;
  current_level: Level | null;
  /** 当前答题原因（v2） */
  quiz_reason: QuizReason;
  /** 已用轮次数（v2） */
  round_count: number;
}

/** 使用中页数据视图（get_monitor_state，v2） */
export interface MonitorStateView {
  phase: GatePhase;
  managed_target_name: string | null;
  remaining_sec: number;
  rounds_used: number;
  warning: boolean;
}

/** 统一错误结构（Rust AppError 序列化） */
export interface AppError {
  code: string;
  message: string;
  data?: unknown;
}
