import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** Svelte 5 编译配置（vitePreprocess 支持 TS 语法） */
export default {
  preprocess: vitePreprocess(),
};
