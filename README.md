# 技术方案：答题校验控制台系统（跨平台：Windows / macOS / Android）
项目名称：**AnswerGate 答题门禁控制台**
核心定位：本地独立控制台程序，**答对题目解锁指定软件启动权限**；题库分幼儿园、小学、初中学科；一次打包产出 Windows、macOS、Android 三个平台；无强制云端，支持离线使用。
> 场景说明：家长管控，孩子必须完成对应难度答题，校验通过后，程序才允许启动游戏/视频软件；答错不解锁，可设置重试次数、冷却时间。

## 一、核心需求梳理
### 功能需求
1. 跨平台：Windows、macOS、Android，统一的图形化答题界面（GUI 为主）
2. 题库分层：
   - 幼儿园：常识、简单识图、儿歌、数字1–20、简单颜色形状
   - 小学：语文、数学、英语、科学（按年级细分）
   - 初中：语数英、物化生、史地政
3. 答题流程：启动程序 → 选择学段 → 抽取N道题 → 作答 → 自动判分 → 达到及格线 → 解锁目标软件启动
4. 管控能力：
   - 配置白名单：需要被"解锁启动"的软件路径/包名
   - 答错策略：重试次数、答错冷却、切换题目
   - 记录日志：答题时间、对错、是否解锁成功
5. 离线优先：题库本地JSON，不需要联网；可选远程更新题库（可选）
6. 权限隔离：
   - Windows：启动外部exe
   - macOS：启动 .app 应用
   - Android：跳转启动其他App（包名）

### 非功能需求
- 界面简洁、大按钮、大字体，孩子可独立操作（触屏/鼠标/键盘均可）
- 打包原生程序，不依赖额外运行时（或者内置运行时）
- 题库可自行增删，格式简单
- 防绕过：基础防直接打开目标程序（不能做到系统级强HOOK，是应用层门禁，适合家庭管控，不是企业安全锁）

## 二、整体架构选型（重点：一套代码，三端编译）
### 推荐技术栈：Rust + Tauri（核心）
> 为什么选这套：
> Rust 写核心业务（答题逻辑、判分、进程启动、日志、配置），高性能、跨平台编译；
> Tauri 2.0：可以输出 **Windows exe、macOS app、Android APK**，一套前端代码三端复用。
> ✅ 完美匹配需求：**统一图形化答题界面 + 三端产出**

| 模块 | 技术 | 职责 |
|---|---|---|
| 核心业务内核 | Rust | 题库加载、随机抽题、判分、分数校验、解锁逻辑、日志、配置读写 |
| GUI前端 | Svelte / Vue（轻量） | 图形答题界面：学段选择、答题、判分结果、冷却倒计时 |
| 前后端桥接 | Tauri 2.0 command / event | 前端调用 Rust 命令（抽题、判分、解锁、读配置） |
| 跨平台打包 | Tauri 2.0 | Windows(.exe/.msi)、macOS(.app/.dmg)、Android(.apk) |
| 题库存储 | JSON | 本地题库文件，可单独编辑 |
| 配置文件 | TOML | 设置及格分数、题量、重试次数、解锁软件列表 |
| 日志 | Rust tracing | 输出本地日志文件 |

> 备选方案：纯Rust + egui（原生GUI）；Flutter（三端但原生启动能力弱）；Electron（包体积大，不推荐）。
> 最终选择 Tauri：兼顾包体积、原生启动能力（Rust 侧直接 spawn 进程）和三端打包成熟度。

## 三、项目目录结构
```
AnswerGate/
├── src-tauri/               # Rust后端核心
│   ├── src/
│   │   ├── quiz.rs          # 题库加载、抽题、判分
│   │   ├── unlock.rs        # 跨平台启动外部程序（重点）
│   │   ├── config.rs        # 读取toml配置
│   │   ├── log.rs           # 本地日志
│   │   ├── model.rs         # 结构体定义：Question、Paper、Level
│   │   ├── commands.rs      # Tauri command 层：暴露给前端的能力
│   │   └── main.rs
│   └── gen/                 # Android 工程自动生成目录
├── src/                     # Tauri前端GUI页面
│   ├── App.svelte           # 页面路由：选学段 → 答题 → 结果/冷却
│   ├── QuizPage.svelte      # 答题页
│   └── ResultPage.svelte    # 判分结果页（通过/失败/冷却倒计时）
├── resources/
│   ├── question_bank/
│   │   ├── kindergarten.json
│   │   ├── primary.json
│   │   ├── junior.json
│   └── config.toml          # 全局配置
├── tests/                   # Rust 单元测试 + 前端组件测试
├── tauri.conf.json
└── README.md
```

## 四、数据结构设计
### 4.1 题目结构体（JSON题库）
```json
{
  "id": "math_1001",
  "level": "primary",
  "grade": "grade2",
  "subject": "math",
  "type": "single_choice",
  "title": "3+5等于几？",
  "options": ["6", "8", "9"],
  "answer": "8",
  "hint": "3往后数5个数"
}
```
题型支持：单选题、判断题、填空题（字符串匹配）
> 移动端适配：手机上填空题输入麻烦，Android 端抽题时优先单选、判断题。

### 4.2 config.toml 配置示例
```toml
[quiz]
default_level = "primary"
question_count = 5       # 一次抽5题
pass_score = 0.8         # 正确率≥80%解锁
max_retry = 2            # 答错整套最多重试2次
cool_down_sec = 120      # 重试耗尽冷却2分钟

[target_apps]
# Windows: exe路径
win = ["C:\\Program Files\\XXX\\game.exe"]
# macOS: app路径
mac = ["/Applications/Minecraft.app"]
# Android: 目标包名
android = ["com.mojang.minecraftpe"]

[log]
enable = true
path = "./answer_gate.log"
```

## 五、核心流程（GUI 答题界面）
```
启动 AnswerGate
   │
   ▼
[学段选择页] 幼儿园 / 小学 / 初中（大按钮卡片）
   │
   ▼
[答题页] 随机抽取 N 道题，逐题作答
   │  · 单选：点击选项；判断：对/错按钮；填空：输入框
   │  · 顶部进度条：第 x/N 题
   ▼
[判分] Rust 侧计算正确率
   │
   ├─ 正确率 ≥ 及格线 ──► [结果页 ✅] 显示"校验通过，即将启动"
   │                        └─► 调用 unlock 模块，按当前系统启动对应软件
   │                        └─► 程序自动退出
   │
   └─ 正确率不足 ──► [结果页 ❌] 显示剩余重试次数，可点击"再试一次"
                        └─ 重试用完 ──► [冷却页 ⏳] 倒计时 120 秒，期间无法答题
```
状态机：`IDLE → QUIZ → GRADING → (PASS → LAUNCH → EXIT) | (FAIL → RETRY / COOLDOWN → QUIZ)`
> 重试、冷却状态由 Rust 侧维护并持久化到日志，防止重启程序绕过冷却。

## 六、跨平台解锁模块 unlock.rs（关键差异）
> 这一段是平台差异化代码，Rust 用 `cfg(target_os)` 编译条件区分
1. **Windows**
   - 使用 `std::process::Command` 直接 spawn exe
   - 注意：路径转义、管理员权限提示；仅应用层启动，不能阻止用户手动打开exe（应用层门禁）
2. **macOS**
   - 不直接执行app内二进制，使用 `open -a "xxx.app"` 命令启动
   - sandbox打包权限注意：macOS app默认有沙箱，想要启动外部程序需要在tauri配置声明权限
3. **Android**
   - Android没有"启动exe"概念，是包名Intent跳转
   - Tauri Android调用 Android JNI / intent 启动目标App
   - 限制：Android 11+ 需要在 `AndroidManifest.xml` 声明 `queries` 包名白名单，否则无法检测、跳转

> ⚠️ 重要边界说明：
> 这个方案属于**应用层门禁**：程序不接管系统底层，孩子如果绕开 AnswerGate，直接自己点开游戏，依然可以打开。
> 如果想要强管控（禁止直接打开）：Windows需要组策略、macOS家长控制、Android系统自带屏幕时间/应用限制，可以和本程序搭配使用。

## 七、打包产出流程
环境：Rust + Node.js + Tauri CLI
```bash
# Windows 打包输出 .exe
tauri build --target x86_64-pc-windows-msvc

# macOS 打包 .app（分别打Intel、Apple Silicon，可合并通用包）
tauri build --target aarch64-apple-darwin
tauri build --target x86_64-apple-darwin

# Android 打包 APK
tauri android build --release
```
输出产物：
- Windows：`AnswerGate.exe` / `.msi`，双击启动 GUI 答题界面
- macOS：`AnswerGate.app`（.dmg），双击启动 GUI 答题界面
- Android：`app-release.apk`，安装后启动 GUI 答题界面

## 八、题库管理方案
1. 题库是独立JSON，和程序本体分离，可以单独维护
2. 内置简易规则：可以按学段、学科、年级过滤抽题
3. 扩展预留：支持后续增加导入Word/Excel题库工具、在线题库同步（可选，非必需）
4. 支持关闭网络：全程离线运行，保护隐私，不需要上传答题记录

## 九、安全与防绕过设计（家庭场景够用）
1. 配置文件、题库可以加简单AES加密（可选，防止孩子直接修改及格线）
2. 日志记录所有答题、解锁事件，家长可以查看
3. 可选密码模式：家长密码，直接跳过答题解锁（应急）
4. 程序本身不常驻后台，**答题→启动软件，之后AnswerGate退出**；不是持续监控程序
5. 重试次数与冷却时间在 Rust 侧持久化，防止重启程序重置状态
> 缺点重申：无法内核拦截手动打开目标软件，适合家庭教育约束，不是专业DRM。

## 十、扩展可选功能（后续迭代）
1. 计时：答题限时
2. 错题本：答错题目存入错题池，下次优先抽错题
3. 多用户：爸爸/妈妈/多个孩子独立档案
4. 统计面板：答题正确率、时长，家长报表
5. 联网版本：题库从内网http服务拉取（校园场景）
6. 锁屏模式：答题界面独占，防止切出去（Android更容易实现，Windows/macOS独占比较麻烦）

## 十一、风险清单 & 避坑
1. macOS 沙箱权限坑：Tauri打包默认沙箱，`open` 外部App会失败，需要修改tauri配置关闭沙箱或者添加权限声明；
2. Android 包名查询限制：Android11+ queries必须显式声明包；
3. Windows Defender：自制Rust程序偶尔会被报风险，需要签名（商业）或者本地信任；
4. 移动端输入：填空题在手机上输入麻烦，优先单选、判断；
5. 前端与 Rust 状态不一致：重试/冷却状态以 Rust 侧为准，前端仅展示，避免刷新页面重置状态。

## 十二、详细执行计划
> 原则：每个阶段有明确交付物与验收标准；阶段内先做可运行的最小闭环，再横向铺开；三端解锁能力是最大风险点，尽早验证（阶段2即触达，不后置）。

### 阶段 0：环境与项目脚手架（0.5 天）
| 任务 | 产出 | 验收标准 |
|---|---|---|
| 安装 Rust stable、Node.js 20+、Tauri 2 CLI、Android SDK/NDK | 开发环境可用 | `cargo --version`、`tauri --version`、`java`、`sdkmanager` 全部可用 |
| `tauri init` 创建项目，接入 Svelte 前端模板 | 项目骨架提交 git | `tauri dev` 能打开空白窗口 |
| 配置 CI（GitHub Actions）：Rust fmt/clippy/test + 前端 lint/tsc | CI 流水线 | 每次 push 自动跑检查并出三端构建产物（Android 可后置到阶段4） |

### 阶段 1：核心业务内核（纯 Rust，不涉及UI）（1 天）
| 任务 | 产出 | 验收标准 |
|---|---|---|
| `model.rs`：Question / Paper / Level / QuizConfig 结构体与 serde 反序列化 | 数据模型 | JSON 样例可正确解析，非法 JSON 报错友好 |
| `quiz.rs`：加载题库、按 level/grade/subject 过滤、随机抽题（去重）、判分 | 抽题判分模块 | 单元测试覆盖：空题库、题目不足N道、全对/全错、正确率边界值 |
| `config.rs`：读取 config.toml，含默认值回退 | 配置模块 | 缺字段/缺文件时使用默认值并可运行 |
| `log.rs`：tracing 落地日志文件 | 日志模块 | 答题事件（时间/学段/得分/是否解锁）完整记录 |

### 阶段 2：解锁模块 + 桌面端竖切（1.5 天）⚠️ 风险最高，提前做
| 任务 | 产出 | 验收标准 |
|---|---|---|
| `unlock.rs` Windows 分支：spawn exe（路径转义、不存在时错误提示） | Windows 启动能力 | 真机测试拉起记事本/游戏 exe |
| `unlock.rs` macOS 分支：`open -a` 启动 .app，处理沙箱权限声明 | macOS 启动能力 | 打包后的 .app 能拉起外部应用（重点验证沙箱配置） |
| `commands.rs`：暴露 `start_quiz / submit_answer / get_result / launch_target` 等 Tauri command | 前后端桥接层 | 前端可调用全部命令，错误通过 Result 返回前端展示 |
| 最简 GUI 串通：学段选择 → 逐题作答 → 判分 → 启动目标程序（样式可粗糙） | **可用的桌面端最小闭环** | 手动完整走通一次"答题→解锁启动"流程 |

### 阶段 3：GUI 完整化（2 天）
| 任务 | 产出 | 验收标准 |
|---|---|---|
| 结果页：通过/失败/剩余重试次数展示 | 完整答题状态机 | 状态机全路径测试：PASS、FAIL→RETRY、RETRY耗尽→COOLDOWN |
| 冷却页：倒计时展示，倒计时由 Rust 侧计时器驱动 | 冷却逻辑 | 重启程序后冷却状态不重置（持久化验证） |
| 视觉打磨：大按钮、大字体、儿童友好配色；答错题目抖动/提示动效 | 可交付 UI | 家长视角走查 + 1080p/2K/触屏分辨率适配 |
| 移动端适配：Android 端抽题自动过滤填空题，布局竖屏适配 | 移动端可用 UI | Android 模拟器上完整答题流程无遮挡、可操作 |

### 阶段 4：Android 端解锁 + 三端打包（1.5 天）
| 任务 | 产出 | 验收标准 |
|---|---|---|
| Android Intent 跳转：JNI 调用启动目标包名，`AndroidManifest.xml` 声明 `queries` 白名单 | Android 启动能力 | 真机（Android 11+）验证跳转启动目标 App |
| 三端打包脚本化：Windows .exe/.msi、macOS .dmg（lipo 通用包）、Android .apk | 打包脚本 + 产物 | 一条命令/脚本产出三端安装包 |
| 安装包真机验收：三平台各一台设备完整走一遍答题→解锁 | 验收记录 | 三端均通过；Defender/Gatekeeper 提示有处理方案（本地信任说明） |

### 阶段 5：题库内容填充（0.5~1 天，可与阶段4并行）
| 任务 | 产出 | 验收标准 |
|---|---|---|
| 幼儿园题库：常识/识图/数字/颜色形状，≥50 题 | kindergarten.json | JSON schema 校验通过，无重复 id |
| 小学题库：语文/数学/英语/科学 × 年级，≥100 题 | primary.json | 同上 |
| 初中题库：语数英、物化生、史地政，≥100 题 | junior.json | 同上 |
| 题库校验脚本（`tests/check_bank.rs` 或独立工具） | 校验工具 | CI 中自动校验题库格式与答案合法性 |

### 阶段 6：测试、加固与发布（1 天）
| 任务 | 产出 | 验收标准 |
|---|---|---|
| 安全加固：家长密码应急解锁（可选开关）；重试/冷却状态持久化 | 加固后的版本 | 密码正确才可跳过；删除配置文件无法重置冷却 |
| 全量回归：Rust 单元测试 + 前端组件测试 + 三端手动回归清单 | 回归测试报告 | `cargo test` + `tsc -b` 全绿；回归清单逐项打勾 |
| 文档：README 使用说明（家长配置指南：改白名单/改及格线/换题库） | 用户文档 | 非技术家长按文档可独立完成配置 |
| v1.0 发布：GitHub Release 挂三端安装包 | v1.0 tag + Release | 三端产物可下载、可安装、可运行 |

### 里程碑总览
```
阶段0 脚手架 ──► 阶段1 内核 ──► 阶段2 解锁+竖切 ──► 阶段3 GUI ──► 阶段4 Android+打包 ──► 阶段6 加固发布
（0.5天）      （1天）        （1.5天）⚠️风险最高    （2天）      （1.5天）  │                    （1天）
                                                              阶段5 题库填充（并行，0.5~1天）
```
- **总工期预估：7~8.5 个工作日**（单人全职）
- **M1（阶段2结束）**：桌面端可演示最小闭环 —— 最早可拿真机验证"答题→拉起软件"
- **M2（阶段4结束）**：三端安装包齐备
- **M3（阶段6结束）**：v1.0 发布

### 关键依赖与并行策略
1. **Android SDK/NDK 环境**在阶段0就装好，避免阶段4卡环境；
2. 阶段2的 macOS 沙箱问题若阻塞，临时用 `tauri dev`（非沙箱）先通流程，沙箱配置单独攻关；
3. 题库填充（阶段5）无技术依赖，可与阶段4并行，也可外包给家长/众包收集；
4. 每阶段结束打 git tag，出对应平台的冒烟安装包，避免问题积累到最后统一暴露。

---

## 十三、家长快速上手（使用指南）

> 本节面向非技术家长。按下面步骤即可完成日常管理，无需了解编程。

### 13.1 安装与启动

- **Windows**：双击 `AnswerGate.exe`（或安装 `.msi`）。首次运行如被 Windows Defender 拦截，点击「仍要运行 / 允许」即可（自制程序未付费签名，属正常提示）。
- **macOS**：打开 `AnswerGate.app`（首次在「系统设置 → 隐私与安全性」点「仍要打开」）。
- **Android**：安装 `app-release.apk`（需允许"安装未知应用"）。

孩子打开程序后只能看到答题界面：选学段 → 答题 → 达标自动启动目标软件并退出，答错则按您设置的规则重试/冷却。

### 13.2 修改白名单 / 题量 / 及格线 / 家长密码

配置文件位置（下称**运行目录**）：
- Windows：`C:\Users\<用户名>\AppData\Roaming\com.answergate.app\config.toml`
- macOS：`~/Library/Application Support/com.answergate.app/config.toml`
- Android：应用私有目录（建议在电脑上改好配置随包发布）

用任意文本编辑器（记事本/文本编辑）打开 `config.toml`：

```toml
[quiz]
question_count = 5        # 每次抽几道题
pass_score = 0.8          # 及格线：正确率 ≥ 80% 解锁（0.6 表示 60%）
max_retry = 2             # 答错整套后可重试几次
cool_down_sec = 120       # 重试用完后冷却多少秒

[[target_apps]]           # 白名单：想管控哪个软件就加一段
id = "minecraft"          # 唯一编号，别重复
name = "我的世界"          # 孩子看到的名字
mac = "/Applications/Minecraft.app"      # macOS 填 .app 路径
win = "C:\\Program Files\\...\\game.exe" # Windows 填 exe 路径
android = "com.mojang.minecraftpe"       # Android 填包名
```

**家长密码（应急解锁）**：在 `config.toml` 末尾加上：

```toml
[parent]
password_hash = "把SHA256哈希粘贴到这里"
```

哈希可用在线工具搜索"SHA256 在线计算"生成（如密码 `1234` 对应 `81dc9bdb52d04dc20036dbd8313ed05581dc9bdb52d04dc20036dbd8313ed055` 的 64 位完整形式）。流程卡住时在程序内输入该密码即可直接解锁。**默认不填即不启用。**

> 改完保存后重启程序生效。**删除 config.toml 不会重置冷却**——冷却状态存放在独立的隐藏文件里，重启/删配置都绕不过。

### 13.3 添加或修改题目

题库是纯文本 JSON 文件，位置：
- 安装包内置：程序资源目录下 `resources/question_bank/`（kindergarten / primary / junior 三个文件）
- 每道题一个 `{...}` 块，照抄现有格式改文字即可
- 注意：`id` 不能重复；单选题 `answer` 必须是 `options` 里的原文；判断题答案只能是「对」或「错」；填空题多个可接受答案用 `|` 隔开
- 改错了也不用怕：程序启动时会校验并明确提示哪一题格式不对

Android 改白名单包名的限制：Android 11+ 系统要求**打包时**声明可跳转的包名清单，因此**新增 android 包名后需重新打包 APK** 才能生效（桌面端改配置即可，无需重打包）。

### 13.4 查看答题日志

日志按天滚动，位于运行目录 `logs/answer_gate.log`（可在 config.toml `[log]` 段关闭或改路径）。每行记录：时间、学段、得分、是否通过、是否解锁成功，家长可直接打开阅读，答题情况一目了然。

### 13.5 管控边界（重要）

本程序是**应用层门禁**：它代为启动白名单软件，但**不能**阻止孩子绕过它直接点开游戏图标。如需更强管控，请配合系统自带家长控制：
- Windows：Microsoft 家庭安全（限制应用与时长）
- macOS：屏幕使用时间（App 限额）
- Android：系统数字健康/屏幕时间

建议把目标软件的图标收进不显眼的文件夹，让孩子养成"先答题再玩"的习惯。

> 📌 **v3 起桌面端已能拦截「绕过门禁」**：watchdog 会发现孩子自己点开的白名单软件，抢前台要求答题后才纳入计时（详见 §15）。
> 但这仍然是应用层管控——孩子若关闭 AnswerGate 进程、或把目标软件移出白名单，仍可绕过；系统级家长控制依然是更硬的兜底。

---

## 十四、使用时长管控（v2 新增：回合制使用教练）

> v2 起 AnswerGate 变为**常驻应用**：答题通过后不再退出，而是持续盯住目标软件的使用时长——到点自动关闭目标软件并要求答题，答对可续期进入下一轮。

### 14.1 轮次制玩法（孩子视角）

```
答题通过 ──► 目标软件打开，「使用中」页开始倒计时 use_minutes
   │
   ├─ 时间到 ──► 目标软件被自动关闭，AnswerGate 弹回前台 ──► 答超时题
   │              ├─ 答对 ──► 「继续使用」──► 目标软件重新打开，新一轮倒计时
   │              └─ 答错/不答 ──► 目标软件保持关闭（重试/冷却规则与首次答题相同）
   │
   └─ 不想用了 ──► 「提前结束本轮」──► 目标软件关闭，回学段页（不触发答题）
```

- 使用中**手动关闭目标软件**（如 Cmd+Q）会被检测到：本轮作废直接回学段页，**不触发答题**（不惩罚正常退出）。
- 杀掉 AnswerGate 也绕不过：本轮剩余时间已持久化，重启后**按剩余时长继续计时**。
- 桌面端二次启动会被单实例锁拒绝并聚焦已有窗口；点窗口关闭按钮会弹二次确认。

### 14.2 config.toml 新增配置

```toml
[[target_apps]]
id = "bilibili"
name = "哔哩哔哩"
mac = "/Applications/Bilibili.app"
use_minutes = 10          # 单轮可用时长（分钟，支持小数，最小 0.1）

[monitor]
fast_test = false         # ⚠️ 仅测试用！true 时单轮固定 10 秒（忽略 use_minutes）
min_use_minutes = 0.1     # use_minutes 下限，无需修改
max_rounds = 0            # 每次解锁最多几轮（含续期）；0 = 不限
timeout_action = "pause"  # 超时动作：pause = 暂停不退出（默认模板）/ kill = 关闭程序
pause_key_code = 49       # pause 模式模拟按键（macOS 键码，49 = 空格）
```

- `use_minutes` 低于 0.1 会自动回退为 0.1 并记日志（防止误配成 0 导致立刻关软件）。
- ⚠️ **`fast_test` 仅限 QA/开发测试**，开启后孩子实际只能用 10 秒；日常使用请保持 `false`。
- 超时答题的题量/及格线：v2 与首次答题完全一致；**v3 起超时续期卷默认 3 题且必须全对**（见 §15.2）。

### 14.3 两种超时动作：pause（暂停）与 kill（关闭）

**`timeout_action = "pause"`（默认模板，v2.1 软管控）**：

```
时间到 ──► AnswerGate 把哔哩哔哩置前并模拟一次「空格」暂停播放（程序不退出）
        ──► 弹回答题页 ──► 答对 ──► 自动再按一次空格恢复播放，进入新一轮倒计时
                    └─► 答错/不答 ──► 哔哩哔哩保持暂停（走重试/冷却）
```

- **必须在 系统设置 → 隐私与安全性 → 辅助功能 中勾选 AnswerGate**，否则模拟按键会被系统拒绝（日志报 -10004）。v2 会自动回退为 kill 关闭目标软件；**v3 起默认不再回退 kill**（需求：只暂停，不能删除），按键失败时保留进程并由暂停守卫持续尝试压制播放（`pause_fallback_kill = true` 可恢复旧行为）。
- `pause_key_code` 默认 49（空格），适配 Bilibili 等空格暂停的播放器；也可改为其他键码。
- Windows 端固定发送**媒体播放/暂停键**（全局生效，无需焦点），`pause_key_code` 被忽略。
- ⚠️ 软管控边界：pause 模式下目标进程仍存活，孩子理论上可以自己切回去点播放——v3 已用「播放状态探测 + 暂停守卫」堵住这条路径（只在确实还在播时才补发暂停键），但仍建议配合系统家长控制。

**`timeout_action = "kill"`（强管控）**：到点直接关闭目标软件进程，答对后重新拉起。

### 14.4 平台能力差异

| 平台 | pause 模式 | kill 模式 | 说明 |
|---|---|---|---|
| macOS | ✅ 置前+模拟键码（需辅助功能权限；v3 起失败不 kill） | ✅ 信号终止 | 按键经 osascript/System Events |
| Windows | 🌐 全局媒体播放/暂停键（VK_MEDIA_PLAY_PAUSE） | ✅ `taskkill /T /F` 进程树 | 需 Windows 真机回归 |
| Android | 🌐 提示模式：到点抢前台弹答题页，不暂停不强杀 | 🌐 同左 | 系统限制；QA 验收以桌面端为准 |

### 14.5 与 v1 一次性模式的关系

v2 未删除 v1 流程，而是替换了它的出口：答题通过后的"启动并退出"变为"启动并纳管计时"。首次答题、重试、冷却、家长密码应急解锁逻辑全部沿用；v1 的行为差异只有一点——**AnswerGate 保持常驻**，关窗需二次确认（确认退出后剩余时长仍会在下次启动时续算）。

### 14.6 已知边界

- 取不到目标进程句柄时（罕见启动竞态），本轮**放弃纳管**（目标软件已打开但不受管控），记日志 `handle_lost`——宁可不管控，不误杀。
- pause 模式答题失败进入冷却时，目标软件保持暂停但进程存活（软管控语义）；此时孩子若手动切回去自行恢复播放，属于软约束边界，家长可选 kill 模式规避。

## 十五、独立进程监控与强制答题（v3 新增：堵死「绕过门禁」）

> v3 解决 v2 的最大漏洞：**孩子从 Dock 自己点开哔哩哔哩，v2 完全无感知、不计时**。
> v3 新增 watchdog 周期扫描 + 拦截答题 + 严格续期门槛，并按需求把超时动作改为**只暂停、不删除**。

### 15.1 孩子视角的完整闭环

```
【绕过门禁】自己点开 B 站
      │
      ├─ watchdog 3s 内发现（不是本程序拉起的）
      │     ├─ 抢前台弹答题页（AnswerGate 置前）
      │     ├─ 视频立即暂停（空格 toggle），进程不退出
      │     └─ 答题期间切回 B 站点播放？暂停守卫 5s 内再压回去
      │
      ├─ 答对（3 题全对）──► 接管**既有进程**（不重新拉起）
      │                      ──► B 站窗口弹出到前台 + 恢复播放
      │                      ──► 开始新的 10 分钟倒计时
      │
      ├─ 答错 ──► 重试；重试耗尽 ──► 冷却（冷却期视频保持暂停，冷却结束再拦截一次）
      │
【正常纳管中】看到第 10 分钟
      │
      ├─ 时间到 ──► 视频暂停（进程保留，**不 kill**）──► 强制答题（3 题全对）
      ├─ 答对 ──► B 站窗口弹出 + 恢复播放 + 新一轮 10 分钟
      └─ 答错 ──► 重试 / 冷却（视频全程保持暂停，进程不删）
```

### 15.2 config.toml 新增配置

```toml
[[target_apps]]
id = "bilibili"
name = "哔哩哔哩"
mac = "/Applications/bilibili.app"
use_minutes = 10          # 单轮可用时长（分钟）—— 超过 10 分钟强制答题
watch = true              # v3：独立进程监控开关（默认 true）

[monitor]
timeout_action = "pause"        # 超时动作：pause = 只暂停不删除（v3 默认语义）
pause_key_code = 49             # macOS 模拟按键（49 = 空格）
pause_fallback_kill = false     # v3：按键失败也**不**回退 kill（需求：只暂停，不能删除）

watch_enabled = true            # v3：是否启用独立进程监控
watch_interval_sec = 3          # 轮询间隔（秒）
watch_mode = "quiz"             # quiz = 先答题后计时（默认）/ adopt = 直接接管计时
timeup_question_count = 3       # 超时续期卷题量（3 题）
timeup_pass_score = 1.0         # 超时续期卷及格线（1.0 = 必须全对；配置调低会被强制抬回）
```

- `timeup_question_count = 0` 会自动回退为 3（0 题 = 无人可答，等于永久卡死）。
- `timeup_pass_score` **不可放宽**：小于 1.0 会被 sanitize 抬回 1.0（「3 题全对」是硬性要求）。
- `watch_mode = "adopt"` 时不打断当前使用，直接接管既有进程开始计时。
- macOS 的 `mac` 路径需与实际安装位置一致（`bundle_executable_name` 会从 `.app/Contents/Info.plist` 读 `CFBundleExecutable`，用于精确定位主进程、排除 Helper 子进程）。

### 15.3 「只暂停，不能删除」的实现约定

| 场景 | v3 行为 |
|---|---|
| 超时到点 | 置前 + 空格暂停，**进程保留**；仅 `pause_fallback_kill = true` 才回退终止 |
| 超时后入题失败（题库异常） | pause 模式**保留进程**（kill 模式才终止） |
| 答题失败进入冷却 | **不 kill**，由暂停守卫持续维持暂停态 |
|  watchdog 拦截答题失败 | 同上，冷却结束后 watchdog 再次拦截仍在运行的 B 站 |

- 空格是**切换**语义：重复发送会反向恢复播放。因此暂停守卫先用「累计 CPU 时间差值」判定视频是否真在播，**只在确实在播时**才补发暂停键；探测不可用时宁可不发（少一道防线也不能反向恢复播放）。
- 「答对后弹出并开始播放」同理：`pause_state`（Paused/Playing/Unknown）持久化在门禁状态里，恢复时据此决定发不发切换键——视频本来就在播时只置前窗口，绝不误把正在播的视频暂停。

### 15.4 watchdog 判定规则

| 条件 | 结果 |
|---|---|
| 目标进程在跑 + 本程序**未纳管**它 + 门禁处于 Idle | **拦截**（抢前台 + 暂停 + 开卷） |
| 目标进程在跑 + 本程序已纳管 | 不处理（由 using monitor 计时） |
| 门禁处于 Quiz / Grading / Failed / Cooldown / Using | 不处理（不抢状态） |
| 本次已拦截过、尚未完成 | 不处理（防重复触发） |

- 冷却期结束回到 Idle 时会清掉拦截标记，保证 watchdog 能再次拦截（否则会永久失效）。
- Android 端无法枚举其他应用进程（系统限制），watchdog 恒不命中，验收以桌面端为准。

### 15.5 新增事件

| 事件 | 触发时机 | 前端动作 |
|---|---|---|
| `gate://watch-intercepted` | watchdog 拦截到绕过启动的目标 | 取活动试卷 → 进答题页（横幅提示「检测到你绕过了门禁」） |
| `gate://watch-abandoned` | 拦截作废（目标已退出 / 配置变更） | 回学段页并提示 |
| `gate://round-started` | 接管既有进程成功 | 进「使用中」页 |
| `gate://permission-required` | 模拟暂停键发送失败（多为缺「辅助功能」权限） | 顶部常驻橙色告警条，提示家长授权 |

### 15.6 本机真机验证（macOS，2026-11-04）

用已安装的 `/Applications/哔哩哔哩.app` 直接启动做了端到端验证，结论与遗留项如下。

**已验证通过**：

| 项目 | 结果 |
|---|---|
| 主二进制探测 | `CFBundleExecutable=哔哩哔哩`；精确 `pgrep` 命中主进程（如 PID 81765），Helper 位于 `Contents/Frameworks/` 下被天然排除 |
| watchdog 拦截 | 日志 `检测到目标软件被绕过本程序启动，拦截并要求答题 target=哔哩哔哩 pid=81765 event="watch_detected"` |
| 状态机 | 落盘 `phase=quiz` / `quiz_reason=watch` / `watch_intercepted=true` / `watch_pid=81765` |
| 只暂停不删除 | 拦截后 B 站进程全程存活（`kill -0` 为 true），未被终止 |
| 播放状态探测 | libproc 兜底可读到 CPU 时间，日志由「探测不可用」变为 `target_pause_failed` / `guard_repaused` |

**本机环境特殊性（已在代码中适配）**：

1. **`/bin/ps`、`/usr/bin/top` 被系统策略禁用**（`operation not permitted`），只有 `pgrep` 可用。
   → 主进程校验改为「`ps` 优先、`pgrep -fl` 兜底、两者都不可用则采信精确路径命中」；
   → 累计 CPU 时间改为「`ps -o time=` 优先、**libproc `proc_pidinfo` 兜底**」（纯用户态，无需特权）。
2. **WorkBuddy CLI 注入了 `ELECTRON_RUN_AS_NODE=1`**，会让 Electron 应用以 Node 模式启动后立即退出。
   在终端手动启动 B 站前需 `unset ELECTRON_RUN_AS_NODE`（生产环境由 Dock 启动，不受影响）。
3. **日志缓冲丢失**：原 `non_blocking` appender 默认攒 128 行才落盘，进程被杀时日志全丢。
   已改为同步写，每条立即落盘——「进程被 kill / 探测失败」这类最需要排查的日志不再丢。

**CPU 阈值实测数据**（1s 采样窗口，libproc）：

| 状态 | 主进程增量 | 整树增量 |
|---|---|---|
| 首页稳定态（无播放） | `0.040 / 0.040 / 0.040 / 0.119 / 0.072` | `0.167 ~ 0.278` |
| 冷启动加载期 | 最高 `0.844` | 最高 `1.143` |

原阈值 `0.12` 贴着空闲峰值 `0.119` 会误判，已上调到 **`0.25`**（`monitor.rs::CPU_BUSY_THRESHOLD_SEC`）。

**仍需人工完成的验证**：

1. **授予辅助功能权限**：`osascript ... key code 49` 本机返回 `-10004 发生权限违例`，
   导致暂停/恢复播放不生效。请在 系统设置 → 隐私与安全性 → 辅助功能 中勾选要运行的
   AnswerGate 可执行文件；dev 构建为
   `/Users/haizai/project/AnswerGate/src-tauri/target/debug/answergate`
   （重新编译后签名变化可能需要重新授权，正式使用建议打包 `.app` 后授权）。
2. **播放态 CPU 增量校准**：缺权限时无法真正暂停视频做 A/B 对比。授权后如发现
   「视频正在播放但守卫不动作」，说明阈值偏高，需下调 `CPU_BUSY_THRESHOLD_SEC`。
3. Windows 端 `tasklist` CPU Time 列 / `VK_MEDIA_PLAY_PAUSE` 仍需对应真机回归。

