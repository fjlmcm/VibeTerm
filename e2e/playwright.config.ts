// Playwright E2E 配置(architecture §I4)
//
// 策略:
//   - 直接对 Vite dev server(http://localhost:1420)做 web 层 E2E,
//     绕过 Tauri runtime — 简单可跑、覆盖 UI 主要流程。
//   - Tauri 集成 E2E(tauri-driver / WebDriver)在 M10+ 加 —
//     需要 macOS / Windows 各自的 WebDriver runner,工作量大,
//     M9 仅打基础。
//
// 测试前 user 需手动跑 `pnpm tauri dev`(或 webServer auto-start),
// 然后 `pnpm --filter @vibeterm/e2e test`。

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
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
