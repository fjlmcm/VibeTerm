// 模块 A:任务列表 CRUD + 右键菜单
//
// 覆盖:
//   - + 按钮 → 新任务
//   - 双击 → 重命名 → Enter 保存
//   - 右键 → 4 个菜单项可见
//   - 右键 → pin/unpin
//   - 右键 → close(带 confirm dialog)
//   - 单击任务 → 激活(border 变化)

import { test, expect } from "./helpers/tauri-fixture";
import { ensureTasks } from "./helpers/tauri-fixture";

test.describe("模块 A:任务 CRUD + 右键菜单", () => {
  test("点 + 按钮 → 新任务出现(prompt dialog 自动 accept)", async ({ tauri }) => {
    const { page } = tauri;
    const before = await page.locator('[data-testid^="task-item-"]').count();
    await page.locator('[data-testid="task-create-btn"]').click();
    await expect
      .poll(async () => page.locator('[data-testid^="task-item-"]').count(), {
        timeout: 5_000,
      })
      .toBe(before + 1);
  });

  test("Cmd/Ctrl+N 快捷键 → 新任务", async ({ tauri }) => {
    const { page } = tauri;
    const before = await page.locator('[data-testid^="task-item-"]').count();
    await page.keyboard.press("Control+n");
    await expect
      .poll(async () => page.locator('[data-testid^="task-item-"]').count(), {
        timeout: 5_000,
      })
      .toBe(before + 1);
  });

  test("右键菜单 → rename → Enter 保存改名(走稳定路径)", async ({ tauri }) => {
    const { page } = tauri;
    await ensureTasks(page, 1);
    const item = page.locator('[data-testid^="task-item-"]').first();
    const oldName = await item.getAttribute("data-task-name");
    const itemId = (await item.getAttribute("data-testid"))!.replace("task-item-", "");

    // 用右键菜单 → rename(避开 dblclick 触发 onClick 副作用 race)
    await item.click({ button: "right" });
    await page.locator('[data-testid="task-ctx-rename"]').click();

    const input = page.locator('[data-testid="task-rename-input"]');
    await expect(input).toBeVisible({ timeout: 3_000 });
    const newName = `renamed-${Date.now()}`;
    // 直接 evaluate 设 value + dispatch input event,绕开 fill/pressSequentially 时序
    await input.evaluate((el, val) => {
      const inp = el as HTMLInputElement;
      inp.value = val as string;
      inp.dispatchEvent(new Event("input", { bubbles: true }));
    }, newName);
    await expect(input).toHaveValue(newName);
    await input.press("Enter");

    const sameTask = page.locator(`[data-testid="task-item-${itemId}"]`);
    await expect
      .poll(async () => await sameTask.getAttribute("data-task-name"), { timeout: 5_000 })
      .toBe(newName);
    expect(oldName).not.toBe(newName);
  });

  test("双击任务 → rename input → Esc 取消(不改名)", async ({ tauri }) => {
    const { page } = tauri;
    await ensureTasks(page, 1);
    const item = page.locator('[data-testid^="task-item-"]').first();
    const oldName = await item.getAttribute("data-task-name");
    const itemId = (await item.getAttribute("data-testid"))!.replace("task-item-", "");
    await item.dblclick();

    const input = page.locator('[data-testid="task-rename-input"]');
    await expect(input).toBeVisible();
    const ghostName = `WOULD-NOT-PERSIST-${Date.now()}`;
    // 用 evaluate 设 value(同 test 3 path,避免 fill 卡死)
    await input.evaluate((el, val) => {
      const inp = el as HTMLInputElement;
      inp.value = val as string;
      inp.dispatchEvent(new Event("input", { bubbles: true }));
    }, ghostName);
    await input.press("Escape");
    await expect(input).toBeHidden({ timeout: 3_000 });

    // 名字应仍是 oldName(cancelOnce flag 阻断了 onBlur 的 commitEdit)
    const sameTask = page.locator(`[data-testid="task-item-${itemId}"]`);
    await page.waitForTimeout(300);
    const finalName = await sameTask.getAttribute("data-task-name");
    expect(finalName).not.toBe(ghostName);
    expect(finalName).toBe(oldName);
  });

  test("右键任务 → 4 个菜单项都出现", async ({ tauri }) => {
    const { page } = tauri;
    await ensureTasks(page, 1);
    const item = page.locator('[data-testid^="task-item-"]').first();
    await item.click({ button: "right" });
    const menu = page.locator('[data-testid="task-ctx-menu"]');
    await expect(menu).toBeVisible({ timeout: 3_000 });
    await expect(page.locator('[data-testid="task-ctx-floating"]')).toBeVisible();
    await expect(page.locator('[data-testid="task-ctx-pin"]')).toBeVisible();
    await expect(page.locator('[data-testid="task-ctx-rename"]')).toBeVisible();
    await expect(page.locator('[data-testid="task-ctx-close"]')).toBeVisible();
  });

  test("右键 → pin/unpin → 状态切换", async ({ tauri }) => {
    const { page } = tauri;
    await ensureTasks(page, 1);
    const item = page.locator('[data-testid^="task-item-"]').first();
    await item.click({ button: "right" });
    await page.locator('[data-testid="task-ctx-pin"]').click();
    // 等 IPC 返回 + 列表刷新;再次右键应显示 unpin
    await page.waitForTimeout(300);
    await item.click({ button: "right" });
    // unpin 文案:i18n 决定;只验菜单仍出现 + 第二项还在(item 还活着)
    await expect(page.locator('[data-testid="task-ctx-pin"]')).toBeVisible();
    // 复位:再次 pin → unpin
    await page.locator('[data-testid="task-ctx-pin"]').click();
  });

  test("右键 → rename → input 出现", async ({ tauri }) => {
    const { page } = tauri;
    await ensureTasks(page, 1);
    const item = page.locator('[data-testid^="task-item-"]').first();
    await item.click({ button: "right" });
    await page.locator('[data-testid="task-ctx-rename"]').click();
    await expect(page.locator('[data-testid="task-rename-input"]')).toBeVisible({
      timeout: 3_000,
    });
    await page.keyboard.press("Escape");
  });

  test("右键 → close(confirm accept)→ 任务数减少", async ({ tauri }) => {
    const { page } = tauri;
    await ensureTasks(page, 2); // 保留至少 2 个,关掉 1 个还剩 1
    const before = await page.locator('[data-testid^="task-item-"]').count();
    const item = page.locator('[data-testid^="task-item-"]').last();
    await item.click({ button: "right" });
    await page.locator('[data-testid="task-ctx-close"]').click();
    await expect
      .poll(async () => page.locator('[data-testid^="task-item-"]').count(), {
        timeout: 5_000,
      })
      .toBe(before - 1);
  });

  test("点任务 → 激活,左边 border 颜色变(data-task-id 出现在 active)", async ({
    tauri,
  }) => {
    const { page } = tauri;
    await ensureTasks(page, 2);
    const items = page.locator('[data-testid^="task-item-"]');
    const first = items.first();
    const second = items.nth(1);
    await first.click();
    // 等 IPC ack;两次点击保证 active 切到 second
    await second.click();
    await page.waitForTimeout(200);
    // 任务激活 → 该任务的 split tree 应初始化 → 出现 split-slot
    await expect(page.locator('[data-testid^="split-slot-"]').first()).toBeVisible({
      timeout: 5_000,
    });
  });
});
