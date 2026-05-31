// 模块 G:浮窗 entry(floating.html)直接加载验证
//
// 注:在 CDP attach 模式里直接 page.goto floating.html — Tauri webview
// 已在 tauri.localhost 域,可以直接通过 location 跳。
// 跳完原页面就丢了(无法回主窗口),所以放到独立测试组,跑完整体不再用主窗口。

import { test, expect } from "./helpers/tauri-fixture";

test.describe("模块 G:浮窗 entry", () => {
  test("浮窗页面加载 + header 出现 + status-dot 出现", async ({ tauri }) => {
    const { page } = tauri;
    // 直接跳到 floating.html?taskId=0(即使任务 0 不存在,header 容器也会渲染)
    await page.goto("http://tauri.localhost/floating.html?taskId=999");
    await expect(page.locator('[data-testid="floating-header"]')).toBeVisible({
      timeout: 5_000,
    });
    await expect(page.locator('[data-testid="floating-status-dot"]')).toBeVisible();
  });

  test("浮窗右键 → context menu 出现 + 2 个菜单项", async ({ tauri }) => {
    const { page } = tauri;
    await page.goto("http://tauri.localhost/floating.html?taskId=999");
    await expect(page.locator('[data-testid="floating-header"]')).toBeVisible();
    // 在页面中间偏下右键(避开 header)
    const vp = page.viewportSize() ?? { width: 800, height: 600 };
    await page.mouse.move(vp.width / 2, vp.height / 2);
    await page.mouse.click(vp.width / 2, vp.height / 2, { button: "right" });
    await expect(page.locator('[data-testid="floating-ctx-menu"]')).toBeVisible({
      timeout: 3_000,
    });
    await expect(page.locator('[data-testid="floating-ctx-palette"]')).toBeVisible();
    await expect(page.locator('[data-testid="floating-ctx-return-main"]')).toBeVisible();
  });
});
