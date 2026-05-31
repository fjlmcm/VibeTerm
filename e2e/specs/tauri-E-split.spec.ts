// 模块 E:分屏 split
//
//   - 水平分屏按钮 / Cmd+D 等价(但快捷键当前 main.tsx 没绑;只验按钮)
//   - 垂直分屏按钮
//   - 关闭分屏按钮 → 减 1 slot
//   - active slot 切换

import { test, expect } from "./helpers/tauri-fixture";
import { ensureTasks, resetSplitToOneSlot } from "./helpers/tauri-fixture";

test.describe("模块 E:分屏", () => {
  test.beforeEach(async ({ tauri }) => {
    const { page } = tauri;
    await ensureTasks(page, 1);
    await page.locator('[data-testid^="task-item-"]').first().click();
    await resetSplitToOneSlot(page);
  });

  test("初始 1 个 slot;点水平分屏 → 2 个 slot", async ({ tauri }) => {
    const { page } = tauri;
    const slots = page.locator('[data-testid^="split-slot-"]');
    await expect.poll(async () => slots.count()).toBe(1);
    await slots.first().click();
    await page.locator('[data-testid="split-h-btn"]').click();
    await expect.poll(async () => slots.count(), { timeout: 5_000 }).toBe(2);
  });

  test("水平分屏 + 垂直分屏 → 3 个 slot", async ({ tauri }) => {
    const { page } = tauri;
    const slots = page.locator('[data-testid^="split-slot-"]');
    await slots.first().click();
    await page.locator('[data-testid="split-h-btn"]').click();
    await expect.poll(async () => slots.count(), { timeout: 5_000 }).toBe(2);
    await slots.first().click();
    await page.locator('[data-testid="split-v-btn"]').click();
    await expect.poll(async () => slots.count(), { timeout: 5_000 }).toBe(3);
  });

  test("点关闭分屏 → slot 减 1", async ({ tauri }) => {
    const { page } = tauri;
    const slots = page.locator('[data-testid^="split-slot-"]');
    await slots.first().click();
    await page.locator('[data-testid="split-h-btn"]').click();
    await expect.poll(async () => slots.count()).toBe(2);
    await slots.last().click();
    await page.locator('[data-testid="split-close-btn"]').click();
    await expect.poll(async () => slots.count(), { timeout: 5_000 }).toBe(1);
  });

  test("点 slot → data-active=true(focus 切换)", async ({ tauri }) => {
    const { page } = tauri;
    const slots = page.locator('[data-testid^="split-slot-"]');
    await slots.first().click();
    await page.locator('[data-testid="split-h-btn"]').click();
    await expect.poll(async () => slots.count()).toBe(2);
    await slots.first().click();
    await expect(slots.first()).toHaveAttribute("data-active", "true");
    await slots.last().click();
    await expect(slots.last()).toHaveAttribute("data-active", "true");
  });
});
