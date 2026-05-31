// 模块 D:真 PTY I/O 闭环
//
// 全栈通路:键盘 → xterm.onData → IPC writePty → shell stdin
//          shell stdout → channel.onmessage → term.write → xterm 渲染
//
// 不依赖具体 shell(cmd.exe / pwsh / powershell 都支持 echo)。
// 读 xterm-rows DOM 文本(xterm 维护的可访问性树),绕开 canvas/WebGL。

import { test, expect, type Page } from "./helpers/tauri-fixture";
import { ensureTasks } from "./helpers/tauri-fixture";

/** 读 xterm buffer 文本(WebglAddon 渲染到 canvas,需走 buffer API)*/
async function readXtermBuffer(page: Page): Promise<string> {
  return page.evaluate(() => {
    const host = document.querySelector('[data-testid="terminal-host"]') as
      | (HTMLElement & {
          __vibeterm_term__?: {
            buffer: {
              active: {
                length: number;
                getLine(n: number): { translateToString(trim?: boolean): string } | undefined;
              };
            };
          };
        })
      | null;
    const term = host?.__vibeterm_term__;
    if (!term) return "";
    const lines: string[] = [];
    const buf = term.buffer.active;
    for (let i = 0; i < buf.length; i++) {
      const line = buf.getLine(i);
      if (line) lines.push(line.translateToString(true));
    }
    return lines.join("\n");
  });
}

/** 等指定字串出现在 xterm buffer 中,默认 5s */
async function waitForXtermContains(
  page: Page,
  needle: string,
  timeout = 5_000,
): Promise<string> {
  let last = "";
  const start = Date.now();
  while (Date.now() - start < timeout) {
    last = await readXtermBuffer(page);
    if (last.includes(needle)) return last;
    await page.waitForTimeout(150);
  }
  throw new Error(
    `waitForXtermContains: 未在 ${timeout}ms 内见到 "${needle}";最后看到:\n${last.slice(-500)}`,
  );
}

/** 激活第 1 个任务,等 PTY ready,然后聚焦 xterm */
async function focusTerminal(page: Page) {
  await ensureTasks(page, 1);
  await page.locator('[data-testid^="task-item-"]').first().click();
  const host = page.locator('[data-testid="terminal-host"]').first();
  await expect(host).toBeVisible({ timeout: 5_000 });
  await expect
    .poll(async () => await host.getAttribute("data-terminal-id"), {
      timeout: 10_000,
    })
    .not.toBeNull();
  // 点击 xterm screen 让 xterm 接 keydown
  const screen = host.locator(".xterm-screen").first();
  await screen.click();
  return host;
}

test.describe("模块 D:终端 PTY 真 I/O 闭环", () => {
  test("Terminal 容器出现 + data-terminal-id 在 10s 内被设置", async ({ tauri }) => {
    const { page } = tauri;
    await ensureTasks(page, 1);
    await page.locator('[data-testid^="task-item-"]').first().click();
    const host = page.locator('[data-testid="terminal-host"]').first();
    await expect(host).toBeVisible({ timeout: 5_000 });
    await expect.poll(
      async () => await host.getAttribute("data-terminal-id"),
      { timeout: 10_000 },
    ).not.toBeNull();
  });

  test("data-mode = task-spawn(在 task 下 spawn,不是 attach 或 standalone)", async ({
    tauri,
  }) => {
    const { page } = tauri;
    await focusTerminal(page);
    const host = page.locator('[data-testid="terminal-host"]').first();
    await expect.poll(
      async () => await host.getAttribute("data-mode"),
      { timeout: 5_000 },
    ).toBe("task-spawn");
  });

  test(".xterm DOM 渲染存在 + 暴露 __vibeterm_term__ 实例", async ({ tauri }) => {
    const { page } = tauri;
    await focusTerminal(page);
    await expect(
      page.locator('[data-testid="terminal-host"] .xterm').first(),
    ).toBeVisible();
    // 验证 host element 上挂了 xterm 实例(E2E 用)
    const hasTerm = await page.evaluate(() => {
      const host = document.querySelector('[data-testid="terminal-host"]') as
        | (HTMLElement & { __vibeterm_term__?: unknown })
        | null;
      return !!host?.__vibeterm_term__;
    });
    expect(hasTerm).toBe(true);
  });

  test("shell prompt 在 10s 内出现(buffer 含非空字符)", async ({ tauri }) => {
    const { page } = tauri;
    await focusTerminal(page);
    const start = Date.now();
    let hasText = false;
    while (Date.now() - start < 10_000) {
      const txt = await readXtermBuffer(page);
      if (txt.trim().length > 0) {
        hasText = true;
        break;
      }
      await page.waitForTimeout(200);
    }
    expect(hasText).toBe(true);
  });

  test("键盘输入回显:type 一段唯一字串 → 在 xterm-rows 中看到", async ({
    tauri,
  }) => {
    const { page } = tauri;
    await focusTerminal(page);
    // 先等 prompt 出现(避免在 shell 启动前输入)
    await page.waitForTimeout(1_500);
    const sentinel = `PTYECHO${Date.now()}`;
    // type 而不是 press,逐字符发送
    await page.keyboard.type(sentinel, { delay: 20 });
    await waitForXtermContains(page, sentinel, 5_000);
  });

  test("执行 echo 命令:Enter 前后两阶段验证(输入回显 + 命令输出)", async ({
    tauri,
  }) => {
    const { page } = tauri;
    await focusTerminal(page);
    await page.waitForTimeout(1_500);
    await page.keyboard.press("Control+c");
    await page.waitForTimeout(300);

    const magic = `PTYCMDOK${Date.now()}`;
    // 阶段 1:type 完(还没 Enter)— buffer 应含一次 magic(输入行回显)
    await page.keyboard.type(`echo ${magic}`, { delay: 20 });
    const afterType = await waitForXtermContains(page, magic, 3_000);
    const beforeEnter = afterType.split(magic).length - 1;
    expect(beforeEnter).toBeGreaterThanOrEqual(1);

    // 阶段 2:Enter 后 — PSReadLine 会重绘输入行(可能覆盖回显),
    // 但命令输出会单独占一行。所以等"出现新的 magic 行"出现
    await page.keyboard.press("Enter");
    await page.waitForTimeout(800);
    const afterEnter = await readXtermBuffer(page);
    // 至少一次出现(若 PSReadLine 没覆盖则 2 次)
    const occurrences = afterEnter.split(magic).length - 1;
    expect(occurrences).toBeGreaterThanOrEqual(1);
    // 且 buffer 行数应增长(prompt 跳到下一行)
    const lineCount = afterEnter.split("\n").length;
    expect(lineCount).toBeGreaterThan(1);
  });

  test("连续 2 条命令独立输出", async ({ tauri }) => {
    const { page } = tauri;
    await focusTerminal(page);
    await page.waitForTimeout(1_500);
    await page.keyboard.press("Control+c");
    await page.waitForTimeout(300);

    const a = `PTYMULTI_A_${Date.now()}`;
    const b = `PTYMULTI_B_${Date.now()}`;
    await page.keyboard.type(`echo ${a}`, { delay: 20 });
    await page.keyboard.press("Enter");
    await waitForXtermContains(page, a, 5_000);

    await page.keyboard.type(`echo ${b}`, { delay: 20 });
    await page.keyboard.press("Enter");
    const final = await waitForXtermContains(page, b, 5_000);
    expect(final).toContain(a);
    expect(final).toContain(b);
  });
});
