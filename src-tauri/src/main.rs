// AnswerGate 桌面端入口（Windows/macOS/Linux）。
// Windows release 构建隐藏控制台窗口。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    answergate_lib::run()
}
