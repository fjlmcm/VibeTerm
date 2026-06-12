// Playwright E2E 配置(architecture §I4)
//
// 策略:
//   - 直接对 Vite dev server(http://localhost:1420)做 web 层 E2E,
//     绕过 Tauri runtime — 简单可跑、覆盖 UI 主要流程。
//   - Tauri 集成 E2E(真 app,CDP)走 tauri-cdp.config.ts(test:tauri)。
//
// dev server 由下方 webServer 自动拉起(本地 :1420 已有服务时直接复用);
// 跑法:`pnpm --filter @vibeterm/e2e run test:smoke`(CI 同款)。
//
// 注意:devices["Desktop Chrome"] 的 UA 是 Windows —— app 按 UA 判平台,
// 快捷键用例须按 Control+ 组合,别写 Meta+(isMacPlatform()=false)。

import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  fullyParallel: false, // PTY 状态共享;串行更稳
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: "http://localhost:1420",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "pnpm dev",
    cwd: "../web/packages/main",
    url: "http://localhost:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
