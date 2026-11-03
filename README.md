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
- 超时答题的题量/及格线/重试与首次答题**完全一致**（无低难度续期卷）。

### 14.3 两种超时动作：pause（暂停）与 kill（关闭）

**`timeout_action = "pause"`（默认模板，v2.1 软管控）**：

```
时间到 ──► AnswerGate 把哔哩哔哩置前并模拟一次「空格」暂停播放（程序不退出）
        ──► 弹回答题页 ──► 答对 ──► 自动再按一次空格恢复播放，进入新一轮倒计时
                    └─► 答错/不答 ──► 哔哩哔哩保持暂停（走重试/冷却）
```

- **必须在 系统设置 → 隐私与安全性 → 辅助功能 中勾选 AnswerGate**，否则模拟按键会被系统拒绝（日志报 -10004），此时自动**回退为 kill 关闭目标软件**，不放任继续使用。
- `pause_key_code` 默认 49（空格），适配 Bilibili 等空格暂停的播放器；也可改为其他键码。
- Windows 端固定发送**媒体播放/暂停键**（全局生效，无需焦点），`pause_key_code` 被忽略。
- ⚠️ 软管控边界：pause 模式下目标进程仍存活，孩子理论上可以自己切回去点播放——pause 适合作为"提醒+答题"型软约束；需要强管控请用 `timeout_action = "kill"`。

**`timeout_action = "kill"`（强管控）**：到点直接关闭目标软件进程，答对后重新拉起。

### 14.4 平台能力差异

| 平台 | pause 模式 | kill 模式 | 说明 |
|---|---|---|---|
| macOS | ✅ 置前+模拟键码（需辅助功能权限，失败回退 kill） | ✅ 信号终止 | 按键经 osascript/System Events |
| Windows | 🌐 全局媒体播放/暂停键（VK_MEDIA_PLAY_PAUSE） | ✅ `taskkill /T /F` 进程树 | 需 Windows 真机回归 |
| Android | 🌐 提示模式：到点抢前台弹答题页，不暂停不强杀 | 🌐 同左 | 系统限制；QA 验收以桌面端为准 |

### 14.5 与 v1 一次性模式的关系

v2 未删除 v1 流程，而是替换了它的出口：答题通过后的"启动并退出"变为"启动并纳管计时"。首次答题、重试、冷却、家长密码应急解锁逻辑全部沿用；v1 的行为差异只有一点——**AnswerGate 保持常驻**，关窗需二次确认（确认退出后剩余时长仍会在下次启动时续算）。

### 14.6 已知边界

- 仍是应用层门禁：孩子绕开 AnswerGate 直接点开目标软件，v2 无法拦截也无法计时（同 13.5）。
- 取不到目标进程句柄时（罕见启动竞态），本轮**放弃纳管**（目标软件已打开但不受管控），记日志 `handle_lost`——宁可不管控，不误杀。
- pause 模式答题失败进入冷却时，目标软件保持暂停但进程存活（软管控语义）；此时孩子若手动切回去自行恢复播放，属于软约束边界，家长可选 kill 模式规避。

