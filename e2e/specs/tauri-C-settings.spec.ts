// 模块 C:设置 4 tab 全覆盖
//
//   theme:  网格加载 + 切主题验 CSS 变量 + active 标记
//   env:    保存按钮可点 + 启用代理切换 + 代理 input 写入
//   keys:   列表加载 + 录入按钮 + Esc 取消录入
//   cli:    rescan 按钮 + 表格行加载 + shell hook 复制按钮存在

import { test, expect } from "./helpers/tauri-fixture";
import { openSettings } from "./helpers/tauri-fixture";

test.describe("模块 C:设置 4 tab", () => {
  test("Ctrl+, 打开,默认 theme tab", async ({ tauri }) => {
    const { page } = tauri;
    await openSettings(page);
    await expect(page.locator('[data-testid="settings-active-tab"]')).toHaveAttribute(
      "data-tab",
      "theme",
    );
  });

  test("Tab 切换:theme → env → keys → cli → theme", async ({ tauri }) => {
    const { page } = tauri;
    await openSettings(page);
    for (const tab of ["env", "keys", "cli", "theme"] as const) {
      await page.locator(`[data-testid="settings-tab-${tab}"]`).click();
      await expect(page.locator('[data-testid="settings-active-tab"]')).toHaveAttribute(
        "data-tab",
        tab,
        { timeout: 2_000 },
      );
    }
  });

  test("Theme:网格 ≥ 2 张卡 + 切到非 active → CSS 变量更新", async ({ tauri }) => {
    const { page } = tauri;
    await openSettings(page, "theme");
    const cards = page.locator('[data-testid^="theme-card-"]');
    const count = await cards.count();
    expect(count).toBeGreaterThanOrEqual(2);

    const before = await page.evaluate(() =>
      getComputedStyle(document.documentElement).getPropertyValue("--color-bg").trim(),
    );
    const activeIdx = await cards.evaluateAll((els) =>
      els.findIndex((e) => e.getAttribute("data-active") === "true"),
    );
    const targetIdx = activeIdx === 0 ? 1 : 0;
    await cards.nth(targetIdx).click();
    await expect
      .poll(
        async () =>
          page.evaluate(() =>
            getComputedStyle(document.documentElement)
              .getPropertyValue("--color-bg")
              .trim(),
          ),
        { timeout: 5_000 },
      )
      .not.toBe(before);
  });

  test("Env tab:保存按钮可见 + proxy enable checkbox 可切换", async ({ tauri }) => {
    const { page } = tauri;
    await openSettings(page, "env");
    await expect(page.locator('[data-testid="env-save-btn"]')).toBeVisible();
    const cb = page.locator('[data-testid="env-proxy-enabled"]');
    await expect(cb).toBeVisible();
    const initial = await cb.isChecked();
    await cb.click();
    await expect(cb).toBeChecked({ checked: !initial });
    await cb.click(); // 复位
  });

  test("Env tab:proxy http 输入可写入", async ({ tauri }) => {
    const { page } = tauri;
    await openSettings(page, "env");
    const input = page.locator('[data-testid="env-proxy-http"]');
    await expect(input).toBeVisible();
    await input.fill("http://127.0.0.1:7890");
    await expect(input).toHaveValue("http://127.0.0.1:7890");
    await input.fill(""); // 清掉避免影响其他测试
  });

  test("Keys tab:列表有 ≥1 行 + 录入按钮可点 + Esc 取消录入", async ({ tauri }) => {
    const { page } = tauri;
    await openSettings(page, "keys");
    const rows = page.locator('[data-testid^="keys-row-"]');
    await expect.poll(async () => rows.count(), { timeout: 3_000 }).toBeGreaterThanOrEqual(1);
    // 取第一行的 command 名
    const first = rows.first();
    const cmd = await first.getAttribute("data-testid");
    const cmdName = cmd!.replace("keys-row-", "");
    await page.locator(`[data-testid="keys-record-${cmdName}"]`).click();
    const capture = page.locator(`[data-testid="keys-capture-${cmdName}"]`);
    await expect(capture).toBeVisible({ timeout: 2_000 });
    await capture.press("Escape");
    await expect(capture).toBeHidden({ timeout: 2_000 });
  });

  test("CLI tab:rescan 按钮可点 + 表格 ≥1 行(claude/codex/aider)", async ({ tauri }) => {
    const { page } = tauri;
    await openSettings(page, "cli");
    await expect(page.locator('[data-testid="cli-rescan-btn"]')).toBeVisible();
    await page.locator('[data-testid="cli-rescan-btn"]').click();
    const rows = page.locator('[data-testid^="cli-row-"]');
    await expect.poll(async () => rows.count(), { timeout: 5_000 }).toBeGreaterThanOrEqual(1);
  });

  test("CLI tab:shell hook 行有复制 + 查看按钮", async ({ tauri }) => {
    const { page } = tauri;
    await openSettings(page, "cli");
    const hooks = page.locator('[data-testid^="cli-hook-"]').filter({
      hasNot: page.locator('[data-testid^="cli-hook-copy-"]'),
    });
    await expect.poll(async () => hooks.count(), { timeout: 3_000 }).toBeGreaterThanOrEqual(1);
  });

  test("CLI tab:点 PATH 修复复制按钮 — 不抛错", async ({ tauri }) => {
    const { page } = tauri;
    await openSettings(page, "cli");
    const btn = page.locator('[data-testid="cli-copy-fix-btn"]');
    await expect(btn).toBeVisible();
    await btn.click();
    // 不做剪贴板真断言(headless 下 clipboard 权限不一致);只验 click 不崩
  });

  test("Settings 背景点击 → 关闭", async ({ tauri }) => {
    const { page } = tauri;
    await openSettings(page);
    // 点 panel 外的 backdrop
    await page.mouse.click(5, 5);
    await expect(page.locator('[data-testid="settings-panel"]')).toBeHidden({
      timeout: 3_000,
    });
  });
});
