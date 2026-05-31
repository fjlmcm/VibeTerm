// Playwright 配置:Tauri 真集成测试(Windows / CDP attach)
//
// 跑所有 specs/tauri-*.spec.ts 模块化测试套件。

import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./specs",
  testMatch: /tauri-[A-Z]-.+\.spec\.ts/,
  fullyParallel: false,
  workers: 1, // 共享 vibeterm.exe + config 文件,串行
  retries: 0,
  reporter: "list",
  timeout: 60_000,
});
