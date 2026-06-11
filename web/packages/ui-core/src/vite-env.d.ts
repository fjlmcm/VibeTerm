// ui-core 不单独 build —— 源码由 @vibeterm/main 的 Vite 一并编译。
// 这里不引入 vite 依赖,只为 `tsc --noEmit` 补上 import.meta.glob 的最小类型。
// 用处:i18n/index.ts 用 glob 自动发现 locales/*.json。
// 如将来 ui-core 直接依赖 vite,可改为 /// <reference types="vite/client" />。

interface ImportMeta {
  readonly glob: <T = unknown>(
    pattern: string,
    options: { readonly eager: true; readonly import: "default" },
  ) => Record<string, T>;
}

// 副作用 css import(terminal/index.tsx 引 @xterm/xterm/css/xterm.css)的最小类型。
declare module "*.css";
