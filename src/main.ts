import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

/** 前端入口：挂载根组件（Svelte 5 mount API） */
const app = mount(App, {
  target: document.getElementById("app") as HTMLElement,
});

export default app;
