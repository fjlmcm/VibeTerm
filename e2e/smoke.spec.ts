// P0 smoke flows(architecture §I4)
//
// 假定:VibeTerm dev server 已启动在 http://localhost:1420
// 这些测试在 vanilla browser 跑 Web 层(Tauri runtime 由 mock 替代) —
// 主要看 UI 装配、键盘路径、组件可见性。
//
// 完整 Tauri 集成 E2E(driving real Tauri app via tauri-driver)留 M12+。

import { test, expect, type Page } from "@playwright/test";

/** Tauri runtime mock —— 必须 stub event channel,否则 onMount 中第一个 listen() 卡死。
 * 同时给 invoke 一些合理的默认返回,避免组件解构 undefined 报错。 */
async function installTauriMock(page: Page) {
  await page.addInitScript(() => {
    let nextId = 1;
    const fakeTheme = {
      id: "vibe",
      name: "Vibe",
      app: { bg: "#000", text: "#fff", border: "#333", surface: "#111" },
      terminal: { background: "#000", foreground: "#fff", cursor: "#fff" },
      status: { running: "#5a5", waiting: "#fa0", idle: "#888" },
    };
    const defaults: Record<string, unknown> = {
      get_config: { active_theme: "vibe", language: "zh-CN" },
      get_theme: fakeTheme,
      list_tasks: [],
      detect_ai_clis: [
        { name: "claude", installed: false },
        { name: "codex", installed: false },
        { name: "aider", installed: false },
      ],
      list_themes: [fakeTheme],
      get_keybindings: [],
      get_env: {},
      get_prompts: [],
    };
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
      invoke: (cmd: string) => {
        if (cmd && cmd.startsWith("plugin:event|")) return Promise.resolve(nextId++);
        if (cmd in defaults) return Promise.resolve(defaults[cmd]);
        return Promise.resolve(null);
      },
      transformCallback: (cb: unknown) => cb,
    };
  });
}

test.describe("VibeTerm Web Smoke", () => {
  test.beforeEach(async ({ page }) => {
    await installTauriMock(page);
  });

  test("main 窗口加载 + header 显示 VibeTerm", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("strong", { hasText: "VibeTerm" })).toBeVisible({
      timeout: 5000,
    });
  });

  test("Cmd+K 打开命令面板,Esc 关闭", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("strong", { hasText: "VibeTerm" })).toBeVisible();
    await page.keyboard.press("Meta+k");
    const input = page.locator('[data-testid="palette-input"]');
    await expect(input).toBeVisible({ timeout: 3000 });
    await expect(input).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(input).toBeHidden({ timeout: 2000 });
  });

  test("Cmd+, 打开设置面板", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("strong", { hasText: "VibeTerm" })).toBeVisible();
    await page.keyboard.press("Meta+,");
    // Settings 组件应该出现 — 用一个能区分的稳定 marker
    // (Settings 顶部有标题 link / 主题选择网格)
    const settingsRoot = page.locator('[data-testid="settings-panel"]');
    await expect(settingsRoot).toBeVisible({ timeout: 3000 });
  });

  test("空任务态:显示 'create a task' 提示", async ({ page }) => {
    await page.goto("/");
    // empty state 文案(中英文都接受)
    await expect(page.locator("main")).toContainText(/create a task|创建任务|新建任务/i, {
      timeout: 5000,
    });
  });

  test("浮窗 entry 加载 + header 显示任务名", async ({ page }) => {
    await page.goto("/floating.html?taskId=0");
    await expect(page.locator("header")).toBeVisible({ timeout: 5000 });
  });

  test("AI CLI banner:全部未装时出现", async ({ page }) => {
    await page.goto("/");
    // 三个 CLI 全 not installed,banner 应该出现
    // 文案含 "CLI" 或 "AI"(中英日都有)
    const banner = page.locator('[data-testid="cli-banner"]');
    await expect(banner).toBeVisible({ timeout: 3000 });
  });
});
