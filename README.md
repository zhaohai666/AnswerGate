# 技术方案：答题校验控制台系统（跨平台：Windows / macOS / Android）
项目名称：**AnswerGate 答题门禁控制台**
核心定位：本地独立控制台程序，**答对题目解锁指定软件启动权限**；题库分幼儿园、小学、初中学科；一次打包产出 Windows、macOS、Android 三个平台；无强制云端，支持离线使用。
> 场景说明：家长管控，孩子必须完成对应难度答题，校验通过后，程序才允许启动游戏/视频软件；答错不解锁，可设置重试次数、冷却时间。

## 一、核心需求梳理
### 功能需求
1. 跨平台：Windows、macOS、Android，统一控制台界面（控制台CLI+简易GUI双模式，控制台优先）
2. 题库分层：
   - 幼儿园：常识、简单识图、儿歌、数字1–20、简单颜色形状
   - 小学：语文、数学、英语、科学（按年级细分）
   - 初中：语数英、物化生、史地政
3. 答题流程：选择学段 → 抽取N道题 → 作答 → 自动判分 → 达到及格线 → 解锁目标软件启动
4. 管控能力：
   - 配置白名单：需要被“解锁启动”的软件路径/包名
   - 答错策略：重试次数、答错冷却、切换题目
   - 记录日志：答题时间、对错、是否解锁成功
5. 离线优先：题库本地JSON，不需要联网；可选远程更新题库（可选）
6. 权限隔离：
   - Windows：启动外部exe
   - macOS：启动 .app 应用
   - Android：跳转启动其他App（包名）

### 非功能需求
- 控制台为主体（stdout/stderr输出，适合脚本调用），附带轻量GUI方便孩子操作
- 打包原生程序，不依赖额外运行时（或者内置运行时）
- 题库可自行增删，格式简单
- 防绕过：基础防直接打开目标程序（不能做到系统级强HOOK，是应用层门禁，适合家庭管控，不是企业安全锁）

## 二、整体架构选型（重点：一套代码，三端编译）
### 推荐技术栈：Rust + Tauri（核心）
> 为什么选这套：
> Rust 写核心业务（答题逻辑、判分、进程启动、日志、配置），高性能、跨平台编译；
> Tauri：可以输出 **Windows exe、macOS app、Android APK**；
> 同时内置 **命令行（控制台）模式**：程序启动时加 `--cli` 参数直接进入纯控制台交互，不加则打开GUI界面。
> ✅ 完美匹配你的需求：**控制台系统界面 + 可视化界面双形态，一套代码三端产出**

| 模块 | 技术 | 职责 |
|---|---|---|
| 核心业务内核 | Rust | 题库加载、随机抽题、判分、分数校验、解锁逻辑、日志、配置读写 |
| 控制台交互 | Rust clap + inquire | 纯CLI模式：终端里选择题段、答题、输入答案、输出结果 |
| GUI前端 | Svelte / Vue（轻量） | 图形答题界面（备选，孩子友好） |
| 跨平台打包 | Tauri 2.0 | Windows(.exe)、macOS(.app)、Android(.apk) |
| 题库存储 | JSON | 本地题库文件，可单独编辑 |
| 配置文件 | TOML | 设置及格分数、题量、重试次数、解锁软件列表 |
| 日志 | Rust tracing | 输出本地日志文件 |

> 备选方案（如果你只想要纯控制台、不要GUI）：纯Rust + clap，GUI完全砍掉，但是Android纯控制台体验很差，Android建议保留简易GUI。
> ❌ 不推荐 Electron：包体积巨大；Flutter：控制台模式比较别扭；Go：UI生态弱，Tauri是当前兼顾CLI+GUI+三端打包最优。

## 三、项目目录结构
```
AnswerGate/
├── src-tauri/               # Rust后端核心
│   ├── src/
│   │   ├── cli.rs           # 控制台入口逻辑（--cli模式）
│   │   ├── quiz.rs          # 题库加载、抽题、判分
│   │   ├── unlock.rs        # 跨平台启动外部程序（重点）
│   │   ├── config.rs        # 读取toml配置
│   │   ├── log.rs           # 本地日志
│   │   ├── model.rs         # 结构体定义：Question、Paper、Level
│   │   └── main.rs
├── src/                     # Tauri前端GUI页面（答题页面）
├── resources/
│   ├── question_bank/
│   │   ├── kindergarten.json
│   │   ├── primary.json
│   │   ├── junior.json
│   └── config.toml          # 全局配置
├── tests/
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

## 五、核心流程（控制台模式，`AnswerGate --cli`）
```
==== AnswerGate 答题门禁控制台 ====
请选择学段：
1: 幼儿园
2: 小学
3: 初中
> 2

正在随机抽取 5 道小学题目...

【第1题/5 数学】
3+5等于几？
A:6  B:8  C:9
请输入选项：B
✅ 回答正确

【第2题/5 语文】
...

答题结束，得分：4/5，正确率80%
✅ 校验通过！即将启动授权软件
> 调用 unlock 模块，根据当前操作系统启动对应程序

如果不通过：
❌ 正确率不足，剩余重试次数：1
重试用完：
⏳ 冷却中，请120秒后再来答题
```

## 六、跨平台解锁模块 unlock.rs（关键差异）
> 这一段是平台差异化代码，Rust 用 `cfg(target_os)` 编译条件区分
1. **Windows**
   - 使用 `std::process::Command` 直接 spawn exe
   - 注意：路径转义、管理员权限提示；仅应用层启动，不能阻止用户手动打开exe（应用层门禁）
2. **macOS**
   - 不直接执行app内二进制，使用 `open -a "xxx.app"` 命令启动
   - sandbox打包权限注意：macOS app默认有沙箱，想要启动外部程序需要在tauri配置声明权限
3. **Android**
   - Android没有“启动exe”概念，是包名Intent跳转
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
- Windows：`AnswerGate.exe`，支持双击GUI，或者 cmd 执行 `AnswerGate.exe --cli` 进入纯控制台
- macOS：`AnswerGate.app`，终端执行 `open AnswerGate.app --args --cli`
- Android：`app-release.apk`；Android控制台体验差，Android端默认GUI模式

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
4. 控制台字体：Windows cmd中文乱码，程序内部设置UTF-8代码页；
5. 移动端输入：填空题在手机上输入麻烦，优先单选、判断。

## 十二、最简起步开发路线
1. 搭建Tauri2基础项目，先完成 `--cli` 控制台分支（优先做控制台，GUI后置）
2. 实现题库JSON读取、随机抽题、判分逻辑
3. 分别实现三大平台 unlock 启动代码，单独测试每个平台能否拉起软件
4. 编写配置、日志模块
5. 填充基础样例题库（幼儿园、小学、初中）
6. 打包三端安装包测试
7. 开发GUI答题页面
