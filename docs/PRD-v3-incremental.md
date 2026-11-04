# AnswerGate 增量 PRD（v3）— 独立进程监控 + 强制答题 + 只暂停不删除

> 版本：v3（增量自 v2）
> 目标平台：Windows / macOS（Android 为弱管控端，见 §6）
> 状态：已实现并通过自动化测试（`cargo test` 全绿），真机回归项见 §5

---

## 1. 增量背景与推翻项

v2 把 AnswerGate 从「一次性解锁器」升级为「回合制使用教练」，但留下一个**致命漏洞**：

> **孩子从 Dock / 桌面图标自己点开哔哩哔哩 → v2 完全无感知、不计时、不要求答题。**
> 门禁等于形同虚设——只要不经过 AnswerGate，管控全部失效。

v3 推翻 v2 的两处约定：

| v2 约定 | v3 推翻 | 原因 |
|---|---|---|
| 「不能管绕开门禁手动打开的目标软件」（PRD-v2 §3 边界说明） | **能管**：watchdog 周期探测，发现即拦截答题 | 这是 v2 最大的管控漏洞 |
| pause 模式按键失败自动回退 kill；答题失败进冷却即 kill 进程 | **只暂停，不能删除**：任何路径都不终止 B 站进程 | 用户明确要求：视频只暂停，不能删除 |
| 超时续期卷与首次答题同难度（PRD-v2 Q5） | **续期卷 3 题且必须全对** | 用户明确要求：超过 10 分钟需要答三次题才能继续 |

---

## 2. 硬性需求（用户口径，逐条可验收）

| # | 需求 | 验收标准 |
|---|---|---|
| R1 | **独立进程监控** —— 监控哔哩哔哩是否启动，不管谁启动的 | 孩子从 Dock 自己点开 B 站，3s 内被 AnswerGate 发现并抢前台弹答题页 |
| R2 | **10 分钟强制答题** —— 看视频超过 10 分钟强制答题，答对才能继续看 | `use_minutes = 10`；到点视频暂停并弹答题页；续期卷 3 题全对才放行 |
| R3 | **只暂停，不能删除** —— 超时动作必须是 pause，进程不得被 kill | 超时/入题失败/答题失败进冷却，三条路径下 B 站进程均保持存活 |
| R4 | **通过后弹出并开始播放** —— 答对后把 B 站窗口置前并恢复播放 | 答对后 B 站窗口自动弹到前台，视频继续播放，同时开始新一轮 10 分钟倒计时 |

---

## 3. 增量需求池

### P0 — v3.0 必须

| 编号 | 需求名称 | 描述 | 优先级 |
|---|---|---|---|
| P0-01 | 独立进程探测 | 跨平台探测白名单目标是否有实例在运行（macOS 精确 pgrep 主二进制 + 主进程校验；Windows tasklist 按 exe 名过滤；Android 恒不可用） | P0 |
| P0-02 | watchdog 周期线程 | 每 `watch_interval_sec`（默认 3s）扫描一次「在跑但未被本程序纳管」的目标 | P0 |
| P0-03 | 拦截答题流程 | 命中即：抢前台 → 暂停视频（不删进程）→ 开严格档试卷（3 题全对）→ 进入 Quiz{Watch} | P0 |
| P0-04 | 接管既有进程 | 答对后**接管既有 PID**（`adopt_running`）而非重新拉起，避免二次 `open` 打断播放进度 | P0 |
| P0-05 | 严格续期门槛 | 超时续期卷与监控拦截卷均为 3 题、及格线 1.0（全对），且该门槛**不可被配置放宽** | P0 |
| P0-06 | 只暂停不删除 | 超时、答题失败进冷却、入题失败三条路径均保留进程；`pause_fallback_kill` 默认 false | P0 |
| P0-07 | 暂停守卫 | 答题/冷却期间每 5s 检查目标是否仍在播放，**仅在确实在播时**补发暂停键并把答题窗拉回前台 | P0 |
| P0-08 | 恢复播放语义 | 答对后「弹出窗口 + 恢复播放」；依据持久化的 `pause_state` 决定发不发切换键，避免反向暂停 | P0 |

### P1 — v3.x（Should have）

| 编号 | 需求名称 | 描述 | 优先级 |
|---|---|---|---|
| P1-01 | 直接接管模式 | `watch_mode = "adopt"` 时不打断当前使用，直接接管既有进程开始计时 | P1 |
| P1-02 | 播放状态探测 | 基于累计 CPU 时间差值判定「是否在解码播放」，供守卫与恢复逻辑使用 | P1 |
| P1-03 | 重启续算 | 重启后保留 `watch_intercepted` / `watch_pid` / `pause_state`，答题中断不丢语义 | P1 |

### P2 — 后续迭代

| 编号 | 需求名称 | 描述 | 优先级 |
|---|---|---|---|
| P2-01 | 使用报告 | 按天统计各目标的实际使用时长与答题通过率 | P2 |
| P2-02 | 多目标并行 | 同时监控多个白名单目标（当前为单目标） | P2 |

---

## 4. UI 增量

### 4.1 答题页横幅（变更）

| 场景 | 横幅文案 |
|---|---|
| 超时续期 | 「使用时间到！哔哩哔哩 已暂停，答对 3 题（及格线 100%）后可继续使用」 |
| 监控拦截（v3 新增） | 「检测到你绕过了门禁直接打开哔哩哔哩！视频已暂停，答对 3 题（及格线 100%）才能继续观看」 |

### 4.2 结果页（变更）

- 及格线文案随答题原因切换：首次解锁显示 `quiz.pass_score`，超时/拦截显示 100%。
- 监控拦截通过态：「答对啦！已接管目标软件，视频将自动弹出并继续播放…」→ 倒计时 3s 后自动 `launch()` → 后端接管既有进程并恢复播放。
- 超时续期通过态（保持 v2）：「▶ 继续使用」→ `extend_and_relaunch()` → 原地恢复播放。

### 4.3 状态机增量

```
v3 新增（Watch 拦截链路）：
IDLE ──watchdog 命中「在跑但未纳管」──► QUIZ{Watch}（视频已暂停）
  → PASS → ADOPT（接管既有 PID）→ USING（新一轮计时）
  → FAIL → RETRY（视频保持暂停）
        → 重试耗尽 → COOLDOWN（视频保持暂停，进程不删）
                    → 冷却到期 → IDLE（清拦截标记，watchdog 可再次拦截）

v2 链路变更（Timeup 链路）：
USING ──到点──► PAUSE（进程保留）→ QUIZ{Timeup}
  → PASS → RESUME（弹出 + 恢复播放）→ USING（新一轮计时）
  → FAIL → RETRY / COOLDOWN（进程保留，暂停守卫维持暂停）
```

---

## 5. 验收要点（真实端到端）

> ✅ = 自动化测试已覆盖；🌐 = 需真机人工回归。

1. **绕过门禁拦截（R1）**
   - ✅ `v3_watchdog::watch_decision_intercepts_when_target_started_outside_answergate`（判定纯逻辑）
   - ✅ `v3_watchdog::watch_intercept_then_adopt_keeps_existing_pid`（拦截 → 答对 → 接管既有 PID，PID 不变）
   - 🌐 真机：手动点开 B 站 → 3s 内 AnswerGate 抢前台弹答题页，视频同时暂停
2. **10 分钟强制答题（R2）**
   - ✅ `v3_watchdog::timeup_and_watch_use_strict_three_questions_all_correct`（3 题 + 及格线 1.0）
   - ✅ `v3_watchdog::strict_threshold_config_is_sanitized`（配置调低及格线被强制抬回 1.0）
   - 🌐 真机：`use_minutes = 0.1`（6s 档）到点 → 暂停 → 答 3 题全对 → 恢复播放
3. **只暂停不删除（R3）**
   - ✅ `e2e_pause_flow::cooldown_entry_keeps_paused_target_alive`（冷却期进程存活 + 守卫仍生效）
   - ✅ `v3_watchdog::config_defaults_enable_watchdog_with_three_second_interval`（`pause_fallback_kill` 默认 false）
   - 🌐 真机：超时后 `ps` 确认 B 站进程仍在（只是暂停）
4. **通过后弹出并开始播放（R4）**
   - ✅ `v3_watchdog::paused_guard_covers_watch_intercept_scenario`（守卫语义）
   - 🌐 真机：答对瞬间 B 站窗口弹到前台且视频继续播放
5. **防死锁回归**
   - ✅ `v3_watchdog::cooldown_expiry_clears_watch_flags_so_watchdog_can_reintercept`（冷却复位清拦截标记）
6. **回归**：`cargo test` 全绿（113 passed / 0 failed）；`vite build` 与 `svelte-check` 0 error。

### 5.1 本机真机验证补记（macOS，2026-11-04）

用已安装的 `/Applications/哔哩哔哩.app` 直接启动做了端到端验证：

- ✅ **R1**：`event="watch_detected"` 日志命中，门禁状态落盘 `phase=quiz` / `quiz_reason=watch` / `watch_pid=<真实PID>`，B 站主进程被精确 pgrep 定位（Helper 自动排除）。
- ✅ **R3**：拦截全程 B 站进程存活（`kill -0` = true），未被终止。
- ✅ **播放状态探测**：`ps`/`top` 被本机系统策略禁用，libproc 兜底生效。
- 🌐 **R2 / R4 尚未闭环**：本机 `osascript` 返回 `-10004 权限违例`（缺辅助功能授权），暂停键与恢复键都发不出去。需家长在「系统设置 → 隐私与安全性 → 辅助功能」勾选 AnswerGate 可执行文件后重验。
- 🌐 **CPU 阈值**：首页稳定态主进程增量 `0.040~0.119s`、整树 `0.167~0.278s`，冷启动加载期最高 `1.143s`；阈值由 `0.12` 上调为 `0.25`。播放态增量待授权后实测校准。

> 环境注意：WorkBuddy CLI 注入 `ELECTRON_RUN_AS_NODE=1` 会让 Electron 应用以 Node 模式启动后立即退出；终端手动启动 B 站前需 `unset ELECTRON_RUN_AS_NODE`（Dock 启动不受影响）。

---

## 6. 平台能力边界

| 能力 | macOS | Windows | Android |
|---|---|---|---|
| 独立进程探测 | ✅ 精确 pgrep 主二进制路径 | ✅ tasklist 按 exe 名过滤 | ❌ 系统限制，无法枚举其他应用 |
| 置前目标窗口 | ✅ System Events `set frontmost` | ⚠️ `AppActivate(pid)` | ❌ |
| 模拟暂停/恢复 | ✅ osascript 虚拟键码（需辅助功能权限） | ✅ `VK_MEDIA_PLAY_PAUSE` | ❌ |
| 播放状态探测 | ✅ `ps -o time=` CPU 增量 | ✅ tasklist CPU Time 列 | ❌ |

- macOS 必须授予 **系统设置 → 隐私与安全性 → 辅助功能** 权限，否则模拟按键失败。
- Android 端 watchdog 恒不命中，验收以桌面端为准（沿用 v2 降级策略）。

---

## 7. 已知边界与风险

| # | 风险 | 现状处理 |
|---|---|---|
| B1 | 目标路径配置不准 → 探测不到 | macOS 按 `.app/Contents/Info.plist` 的 `CFBundleExecutable` 精确定位主进程；README §15.2 已提示核对安装路径 |
| B2 | CPU 阈值误判（暂停态 Electron 仍跑 UI 动画） | 阈值 `0.12s/1s`，需真机校准；探测不可用时**宁可不发**切换键（少一道防线也不反向恢复播放） |
| B3 | 孩子关掉 AnswerGate 进程 | 仍是应用层管控；本轮剩余时长持久化，重启续算；系统家长控制是更硬兜底 |
| B4 | 空格是切换语义，重复发送会反向恢复播放 | 引入 `PauseState` 三态 + 播放状态探测；恢复播放前先判定 |
