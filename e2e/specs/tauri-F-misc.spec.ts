// 模块 F:AI CLI banner / urgency / 通用 UI

import { test, expect } from "./helpers/tauri-fixture";

test.describe("模块 F:辅助 UI", () => {
  test("AI CLI banner — 若出现,有 dismiss X 按钮可关", async ({ tauri }) => {
    const { page } = tauri;
    const banner = page.locator('[data-testid="cli-banner"]');
    if (await banner.isVisible().catch(() => false)) {
      await page.locator('[data-testid="cli-banner-dismiss"]').click();
      await expect(banner).toBeHidden({ timeout: 3_000 });
    } else {
      test.info().annotations.push({
        type: "skip",
        description: "banner 未显示(可能 AI CLI 全已安装)",
      });
    }
  });

  test("urgency 模式:点 toggle → 按钮 active 背景变 → 任务列表顺序可能变", async ({
    tauri,
  }) => {
    const { page } = tauri;
    const btn = page.locator('[data-testid="urgency-toggle"]');
    const initialBg = await btn.evaluate((el) => (el as HTMLElement).style.background);
    await btn.click();
    await expect
      .poll(async () => btn.evaluate((el) => (el as HTMLElement).style.background))
      .not.toBe(initialBg);
    await btn.click(); // 复位
  });

  test("Header 'VibeTerm' 标题、create / urgency 按钮、task-list 容器都在", async ({
    tauri,
  }) => {
    const { page } = tauri;
    await expect(page.locator("strong", { hasText: "VibeTerm" }).first()).toBeVisible();
    await expect(page.locator('[data-testid="task-create-btn"]')).toBeVisible();
    await expect(page.locator('[data-testid="urgency-toggle"]')).toBeVisible();
    await expect(page.locator('[data-testid="task-list"]')).toBeVisible();
  });

  test("i18n:UI 至少能找到 'VibeTerm' / '任务' / 'task' 任一字串", async ({ tauri }) => {
    const { page } = tauri;
    const bodyText = await page.evaluate(() => document.body.innerText);
    expect(bodyText).toMatch(/VibeTerm|任务|task/i);
  });

  test("CSS 变量 --color-bg / --color-text 注入到 document.documentElement", async ({
    tauri,
  }) => {
    const { page } = tauri;
    const vars = await page.evaluate(() => ({
      bg: getComputedStyle(document.documentElement)
        .getPropertyValue("--color-bg")
        .trim(),
      text: getComputedStyle(document.documentElement)
        .getPropertyValue("--color-text")
        .trim(),
    }));
    expect(vars.bg).not.toBe("");
    expect(vars.text).not.toBe("");
  });

  test("Escape 关闭一切 modal(palette + settings 都打开 → Esc 全关)", async ({
    tauri,
  }) => {
    const { page } = tauri;
    await page.keyboard.press("Control+k");
    await expect(page.locator('[data-testid="palette-input"]')).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator('[data-testid="palette-input"]')).toBeHidden();

    await page.keyboard.press("Control+,");
    await expect(page.locator('[data-testid="settings-panel"]')).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator('[data-testid="settings-panel"]')).toBeHidden();
  });
});
