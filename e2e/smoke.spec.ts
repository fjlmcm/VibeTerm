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
      // 真实 IPC 返回 KeybindingsFile{bindings},不是裸数组 —— 裸数组会让
      // dispatcher 的 kb.bindings 不可迭代,键盘路径全灭。只 stub 用到的两条。
      get_keybindings: {
        bindings: [
          { command: "command_palette", keys: "Mod+K" },
          { command: "open_settings", keys: "Mod+," },
        ],
      },
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
      // @tauri-apps/api 2.11+ 的 getCurrentWindow/getCurrentWebview 同步读这里,
      // 缺了会在 Titlebar mount 时直接抛 TypeError 整页白屏。
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main", windowLabel: "main" },
      },
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
    const input = page.locator('[data-testid="palette-input"]');
    // header 可见 ≠ 全局 keydown 监听已注册(onMount 里一串 await 之后才 addEventListener),
    // 单次按键会与启动竞态 → 按键+断言整体重试,直到监听就位。
    await expect(async () => {
      await page.keyboard.press("Control+k");
      await expect(input).toBeVisible({ timeout: 300 });
    }).toPass({ timeout: 5000 });
    await expect(input).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(input).toBeHidden({ timeout: 2000 });
  });

  test("Cmd+, 打开设置面板", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("strong", { hasText: "VibeTerm" })).toBeVisible();
    // Settings 组件应该出现 — 用一个能区分的稳定 marker
    // (Settings 顶部有标题 link / 主题选择网格)
    const settingsRoot = page.locator('[data-testid="settings-panel"]');
    // 同上:与全局 keydown 监听注册竞态,按键+断言整体重试
    await expect(async () => {
      await page.keyboard.press("Control+,");
      await expect(settingsRoot).toBeVisible({ timeout: 300 });
    }).toPass({ timeout: 5000 });
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

  test("任务右键菜单:Portal 到 body 且整体留在视口内", async ({ page }) => {
    // 回归:菜单曾是裸 fixed 定位 — 贴边右键伸出窗口被裁切,canvas 模式还会陷进
    // transform/stacking context 被遮挡。修复 = Portal 到 body + 实测尺寸视口夹取。
    await page.addInitScript(() => {
      const internals = (window as Window & {
        __TAURI_INTERNALS__?: { invoke: (cmd: string) => Promise<unknown> };
      }).__TAURI_INTERNALS__!;
      const orig = internals.invoke;
      internals.invoke = (cmd: string) => {
        if (cmd === "list_tasks") {
          return Promise.resolve([
            {
              id: 1,
              name: "demo",
              cwd: null,
              pinned: false,
              status: "idle",
              terminal_ids: [],
              location: { kind: "MainWorkspace" },
              split_tree: { kind: "leaf", slot_id: 0 },
              notify_muted: false,
            },
          ]);
        }
        return orig(cmd);
      };
    });
    // 故意压扁窗口:任务行下方放不下菜单,夹取逻辑必须把菜单收回视口
    await page.setViewportSize({ width: 600, height: 220 });
    await page.goto("/");
    const row = page.locator(".task-row");
    await expect(row).toBeVisible({ timeout: 5000 });
    await row.click({ button: "right" });
    const menu = page.locator('[data-testid="task-ctx-menu"]');
    await expect(menu).toBeVisible({ timeout: 2000 });
    // Portal 生效:菜单已逃出侧栏子树(Solid Portal 在 body 下挂容器 div)
    const escaped = await menu.evaluate(
      (el) => !el.closest('[data-testid="task-list"]') && el.parentElement?.parentElement === document.body,
    );
    expect(escaped).toBe(true);
    // 整体留在视口内(含 8px margin 的余量判断放宽到 0/视口边)
    const box = (await menu.boundingBox())!;
    const vp = page.viewportSize()!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(vp.width);
    expect(box.y + box.height).toBeLessThanOrEqual(vp.height);
  });
});
