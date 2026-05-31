// 模块 B:命令面板 — 搜索 / 上下键 / Enter / mouse hover / 关闭

import { test, expect } from "./helpers/tauri-fixture";
import { ensureTasks } from "./helpers/tauri-fixture";

test.describe("模块 B:命令面板", () => {
  test("Ctrl+K 打开,Esc 关闭", async ({ tauri }) => {
    const { page } = tauri;
    await page.keyboard.press("Control+k");
    const input = page.locator('[data-testid="palette-input"]');
    await expect(input).toBeVisible({ timeout: 5_000 });
    // autofocus 在某些 webview 时序下可能滞后;显式 focus 兜底
    await input.focus();
    await expect(input).toBeFocused({ timeout: 5_000 });
    await input.press("Escape");
    await expect(input).toBeHidden({ timeout: 3_000 });
  });

  test("命令面板默认列出任务 + 主题 + 内置命令", async ({ tauri }) => {
    const { page } = tauri;
    await ensureTasks(page, 1);
    await page.keyboard.press("Control+k");
    const list = page.locator('[data-testid="palette-list"]');
    await expect(list).toBeVisible();
    const items = page.locator('[data-testid^="palette-item-"]');
    // 至少有:1 任务 + 多主题 + 新建任务命令 + 设置命令
    await expect.poll(async () => items.count(), { timeout: 3_000 })
      .toBeGreaterThanOrEqual(3);
    // 内置命令"打开设置"应在
    await expect(page.locator('[data-testid="palette-item-cmd:open-settings"]')).toBeVisible();
    await page.keyboard.press("Escape");
  });

  test("输入过滤:打 theme 应只剩主题项", async ({ tauri }) => {
    const { page } = tauri;
    await page.keyboard.press("Control+k");
    const input = page.locator('[data-testid="palette-input"]');
    await input.fill("Tokyo"); // 任意主题名前缀
    await page.waitForTimeout(150);
    const items = page.locator('[data-testid^="palette-item-"]');
    const count = await items.count();
    // 仅匹配该关键字的项
    expect(count).toBeGreaterThan(0);
    // 所有可见项 label 应都含 "Tokyo"
    const labels = await items.allTextContents();
    for (const l of labels) expect(l.toLowerCase()).toContain("tokyo");
    await page.keyboard.press("Escape");
  });

  test("ArrowDown / ArrowUp 切换高亮", async ({ tauri }) => {
    const { page } = tauri;
    await page.keyboard.press("Control+k");
    const input = page.locator('[data-testid="palette-input"]');
    await expect(input).toBeVisible();
    const items = page.locator('[data-testid^="palette-item-"]');
    // 等列表至少 3 项加载完(theme listing 是 async)
    await expect.poll(async () => items.count(), { timeout: 5_000 })
      .toBeGreaterThanOrEqual(3);
    // 初始第一个高亮
    await expect(items.nth(0)).toHaveAttribute("data-highlighted", "true");
    await input.press("ArrowDown");
    await expect(items.nth(1)).toHaveAttribute("data-highlighted", "true");
    await input.press("ArrowDown");
    await expect(items.nth(2)).toHaveAttribute("data-highlighted", "true");
    await input.press("ArrowUp");
    await expect(items.nth(1)).toHaveAttribute("data-highlighted", "true");
    await input.press("Escape");
  });

  test("Enter 触发『打开设置』命令 → 设置面板出现", async ({ tauri }) => {
    const { page } = tauri;
    await page.keyboard.press("Control+k");
    const input = page.locator('[data-testid="palette-input"]');
    await input.fill("打开设置");
    await page.waitForTimeout(150);
    await page.keyboard.press("Enter");
    await expect(page.locator('[data-testid="settings-panel"]')).toBeVisible({
      timeout: 3_000,
    });
  });

  test("命令面板鼠标 hover → 高亮跟随", async ({ tauri }) => {
    const { page } = tauri;
    await page.keyboard.press("Control+k");
    const items = page.locator('[data-testid^="palette-item-"]');
    await items.nth(2).hover();
    await expect(items.nth(2)).toHaveAttribute("data-highlighted", "true");
    await page.keyboard.press("Escape");
  });

  test("无匹配文案显示", async ({ tauri }) => {
    const { page } = tauri;
    await page.keyboard.press("Control+k");
    const input = page.locator('[data-testid="palette-input"]');
    await input.fill("ZZZ_NOTHING_MATCHES_THIS_QUERY_XYZ123");
    await expect(page.locator('[data-testid="palette-list"]')).toHaveAttribute(
      "data-count",
      "0",
    );
    await page.keyboard.press("Escape");
  });
});
