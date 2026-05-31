// 共享 fixture:spawn vibeterm.exe + CDP attach + dialog handler
//
// 给所有 tauri-cdp E2E spec 复用。导出 `test`(扩展自 @playwright/test),
// 每个 worker 起一个独立 vibeterm.exe(端口由 worker_index 错开),
// 测试间复用 page。

import { test as base, chromium, type Browser, type Page } from "@playwright/test";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, type ChildProcess } from "node:child_process";
import http from "node:http";
import fs from "node:fs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..", "..", "..");
const applicationPath = path.join(
  repoRoot,
  "src-tauri",
  "target",
  "release",
  "vibeterm.exe",
);

interface TauriContext {
  page: Page;
  appProc: ChildProcess;
  browser: Browser;
}

async function waitForTauriPage(port: number, timeoutMs: number): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const targets = await new Promise<Array<{ url?: string; type?: string }>>(
        (resolve, reject) => {
          const req = http.get(`http://127.0.0.1:${port}/json`, (res) => {
            let body = "";
            res.on("data", (c) => (body += c));
            res.on("end", () => {
              try {
                resolve(JSON.parse(body));
              } catch (e) {
                reject(e);
              }
            });
          });
          req.on("error", reject);
          req.setTimeout(1000, () => req.destroy(new Error("timeout")));
        },
      );
      const hit = targets.find(
        (t) => t.type === "page" && t.url?.includes("tauri.localhost"),
      );
      if (hit) return;
    } catch {
      /* keep polling */
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error(`tauri.localhost page never appeared on debug port ${port}`);
}

export async function startTauri(port: number): Promise<TauriContext> {
  if (!fs.existsSync(applicationPath)) {
    throw new Error(`找不到 .exe — ${applicationPath}\n先 pnpm tauri build --no-bundle`);
  }
  const appProc = spawn(applicationPath, [], {
    detached: true,
    stdio: "ignore",
    env: {
      ...process.env,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    },
  });
  appProc.unref();
  await waitForTauriPage(port, 30_000);

  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const ctx = browser.contexts()[0];
  if (!ctx) throw new Error("no browser context found over CDP");
  const page =
    ctx.pages()[0] ??
    (await ctx.waitForEvent("page", { timeout: 5_000 }));
  if (!page) throw new Error("no page found");

  // 默认 dialog handler — handleCreateTask 用 window.prompt(),addEnv 也用
  page.on("dialog", async (d) => {
    if (d.type() === "prompt") {
      await d.accept(`e2e-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`);
    } else if (d.type() === "confirm") {
      await d.accept();
    } else {
      await d.dismiss();
    }
  });

  await page.locator("strong", { hasText: "VibeTerm" }).first().waitFor({
    timeout: 15_000,
  });
  return { page, appProc, browser };
}

export async function stopTauri(ctx: TauriContext): Promise<void> {
  try {
    await ctx.browser.close();
  } catch {
    /* ignore */
  }
  if (ctx.appProc.pid) {
    try {
      process.kill(ctx.appProc.pid);
    } catch {
      /* ignore */
    }
  }
}

/** 复位 UI:若在 floating.html 跳回主窗口;Esc 关 modal,等隐藏 */
export async function resetUi(page: Page): Promise<void> {
  // module G 之后 page 可能停在 floating.html,需要跳回 index.html
  if (page.url().includes("floating.html")) {
    await page.goto("http://tauri.localhost/index.html");
    await page
      .locator("strong", { hasText: "VibeTerm" })
      .first()
      .waitFor({ timeout: 10_000 });
  }
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page
    .locator('[data-testid="settings-panel"]')
    .waitFor({ state: "hidden", timeout: 2_000 })
    .catch(() => {});
  await page
    .locator('[data-testid="palette-input"]')
    .waitFor({ state: "hidden", timeout: 2_000 })
    .catch(() => {});
  await page
    .locator('[data-testid="prompt-picker"]')
    .waitFor({ state: "hidden", timeout: 2_000 })
    .catch(() => {});
  await page
    .locator('[data-testid="task-ctx-menu"]')
    .waitFor({ state: "hidden", timeout: 2_000 })
    .catch(() => {});
}

/** 确保至少有 N 个任务存在(用 + 按钮 + prompt dialog handler 创建) */
export async function ensureTasks(page: Page, minCount: number): Promise<number> {
  let current = await page.locator('[data-testid^="task-item-"]').count();
  while (current < minCount) {
    await page.locator('[data-testid="task-create-btn"]').click();
    await page
      .locator('[data-testid^="task-item-"]')
      .nth(current)
      .waitFor({ state: "visible", timeout: 5_000 });
    current = await page.locator('[data-testid^="task-item-"]').count();
  }
  return current;
}

/** 打开设置面板并切到指定 tab */
export async function openSettings(
  page: Page,
  tab: "theme" | "env" | "keys" | "cli" = "theme",
): Promise<void> {
  // 若已打开就不重复触发(避免 Ctrl+, 切换)
  const panel = page.locator('[data-testid="settings-panel"]');
  if (!(await panel.isVisible().catch(() => false))) {
    await page.keyboard.press("Control+,");
    await panel.waitFor({ state: "visible", timeout: 5_000 });
  }
  if (tab !== "theme") {
    await page.locator(`[data-testid="settings-tab-${tab}"]`).click();
  }
  // settings-active-tab marker 是 display:none(隐藏 marker),用 attached 等
  await page
    .locator('[data-testid="settings-active-tab"]')
    .waitFor({ state: "attached", timeout: 2_000 });
}

/** 把指定任务的 split tree 重置回 1 个 slot(逐个关闭多余 slot)*/
export async function resetSplitToOneSlot(page: Page): Promise<void> {
  const slots = page.locator('[data-testid^="split-slot-"]');
  let count = await slots.count();
  let guard = 0;
  while (count > 1 && guard++ < 10) {
    await slots.last().click();
    await page.locator('[data-testid="split-close-btn"]').click();
    await page.waitForTimeout(150);
    count = await slots.count();
  }
}

/** Worker scoped fixture(每个 test worker 1 个 vibeterm 实例) */
export const test = base.extend<{ tauri: TauriContext }, { tauriWorker: TauriContext }>({
  tauriWorker: [
    async ({}, use, workerInfo) => {
      const port = 9223 + workerInfo.workerIndex;
      const ctx = await startTauri(port);
      await use(ctx);
      await stopTauri(ctx);
    },
    { scope: "worker", auto: false },
  ],
  tauri: async ({ tauriWorker }, use) => {
    await resetUi(tauriWorker.page);
    await use(tauriWorker);
    await resetUi(tauriWorker.page);
  },
});

export { expect } from "@playwright/test";
