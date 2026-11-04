import { mount } from "svelte";
import App from "./App.svelte";
import { api } from "./lib/api";
import "./app.css";

/** 前端全局错误上报（v2.1）：打包应用的 webview 控制台不可见，
 *  所有未捕获错误/ Promise 拒绝上报到 answer_gate.log，是唯一诊断通道 */
window.onerror = (msg, source, lineno, colno) => {
  void api.frontendLog(
    "error",
    `window.onerror: ${String(msg)} @ ${source}:${lineno}:${colno}`,
  );
};
window.onunhandledrejection = (event) => {
  void api.frontendLog("error", `unhandledrejection: ${String(event.reason)}`);
};

/** 前端入口：挂载根组件（Svelte 5 mount API） */
const app = mount(App, {
  target: document.getElementById("app") as HTMLElement,
});

export default app;
