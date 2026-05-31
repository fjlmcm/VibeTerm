// 模块 H:OSC 133 状态嗅探 — shell 写 OSC 序列 → StatusDetector 解析 → 前端任务状态变更
//
// 全栈通路:
//   pwsh `[Console]::Write([char]0x1b + ']133;A' + [char]0x07)` → PTY stdout
//   → LazyChannelSink.push(chunk) → StatusDetector.feed(chunk)
//   → parse_osc → current = WaitingInput
//   → tasks.update_terminal_status → emit task_status_changed + tasks_changed
//   → 前端 onTasksChanged → TaskList re-render → data-task-status="waiting_input"
//
// 这是 §1 第 0 层 OSC 133 集成在真 Windows pwsh / WebView2 上的端到端验证。

import { test, expect, type Page } from "./helpers/tauri-fixture";
import { ensureTasks } from "./helpers/tauri-fixture";

/** 拿当前激活的第 1 个任务的 id + status,task 列表里第一个 */
async function getFirstTaskStatus(page: Page): Promise<string | null> {
  return page.locator('[data-testid^="task-item-"]').first().getAttribute(
    "data-task-status",
  );
}

/** 拿 xterm buffer 文本(同模块 D) */
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

/** 等 task 状态变成期望值 */
async function expectTaskStatus(
  page: Page,
  expected: "idle" | "running" | "waiting_input",
  timeout = 5_000,
): Promise<void> {
  await expect.poll(() => getFirstTaskStatus(page), { timeout })
    .toBe(expected);
}

/** 在 xterm 中执行一行命令(已聚焦) */
async function execCmd(page: Page, cmd: string): Promise<void> {
  await page.keyboard.type(cmd, { delay: 15 });
  await page.keyboard.press("Enter");
}

/** 激活任务 + 聚焦 xterm + 等 prompt 渲染 */
async function setupTerminal(page: Page): Promise<void> {
  await ensureTasks(page, 1);
  await page.locator('[data-testid^="task-item-"]').first().click();
  const host = page.locator('[data-testid="terminal-host"]').first();
  await expect(host).toBeVisible({ timeout: 5_000 });
  await expect
    .poll(async () => host.getAttribute("data-terminal-id"), {
      timeout: 10_000,
    })
    .not.toBeNull();
  await host.locator(".xterm-screen").first().click();
  // 等 shell prompt 渲染(buffer 非空)
  await expect
    .poll(async () => (await readXtermBuffer(page)).trim().length, {
      timeout: 10_000,
    })
    .toBeGreaterThan(0);
}

test.describe("模块 H:OSC 133 状态嗅探(真 shell → StatusDetector → 前端)", () => {
  test("data-task-status 属性在任务行可见", async ({ tauri }) => {
    const { page } = tauri;
    await setupTerminal(page);
    // 状态属性可能在新任务刚创建时短暂为空,poll 几次
    await expect
      .poll(async () => await getFirstTaskStatus(page), { timeout: 5_000 })
      .toMatch(/^(idle|running|waiting_input)$/);
  });

  test("等任务回到 idle(默认 800ms 无输出后 tick)", async ({ tauri }) => {
    const { page } = tauri;
    await setupTerminal(page);
    // PowerShell 启动后 prompt 输出 → Running → 1s 不动 → tick → Idle
    await page.waitForTimeout(1_500);
    await expectTaskStatus(page, "idle", 8_000);
  });

  test("OSC 133;A → 任务状态 → waiting_input(独特可证伪)", async ({ tauri }) => {
    const { page } = tauri;
    await setupTerminal(page);
    await page.waitForTimeout(1_500);
    // 先确认进入 idle 基线
    await expectTaskStatus(page, "idle", 8_000);

    // pwsh 5.1/7 都支持 [char]0x1b + ... 写法
    // 命令本身没写 OSC,所以只在执行后产生 OSC A 字节
    await execCmd(
      page,
      `[Console]::Write([char]0x1b + ']133;A' + [char]0x07)`,
    );
    await expectTaskStatus(page, "waiting_input", 5_000);
  });

  test("OSC 133;D → idle(强制 idle,不依赖 tick 超时)", async ({ tauri }) => {
    const { page } = tauri;
    await setupTerminal(page);
    // 不假设初始 idle(worker fixture 跨测试共享状态)
    // 先进入 waiting_input,再发 OSC D
    await execCmd(page, `[Console]::Write([char]0x1b + ']133;A' + [char]0x07)`);
    await expectTaskStatus(page, "waiting_input", 8_000);

    // 现在发 OSC D — parse_osc 直接设为 Idle
    await execCmd(page, `[Console]::Write([char]0x1b + ']133;D' + [char]0x07)`);
    // OSC D 后 PowerShell prompt 可能很快又输出 → Running,所以用"曾出现 idle"判定
    let sawIdle = false;
    const start = Date.now();
    while (Date.now() - start < 5_000) {
      if ((await getFirstTaskStatus(page)) === "idle") {
        sawIdle = true;
        break;
      }
      await page.waitForTimeout(100);
    }
    expect(sawIdle).toBe(true);
  });

  test("OSC 133;C → running(显式标记 running)", async ({ tauri }) => {
    const { page } = tauri;
    await setupTerminal(page);
    await page.waitForTimeout(1_500);
    await expectTaskStatus(page, "idle", 8_000);
    await execCmd(page, `[Console]::Write([char]0x1b + ']133;C' + [char]0x07)`);
    // OSC C → Running;但 PowerShell prompt 也会 bump 到 Running,所以这个测试
    // 主要验"OSC C 不会 panic / 解析后状态被更新到 frontend"
    await expectTaskStatus(page, "running", 5_000);
  });

  test("OSC 序列状态机三态转换 (A→C→D)", async ({ tauri }) => {
    const { page } = tauri;
    await setupTerminal(page);
    await page.waitForTimeout(1_500);
    await expectTaskStatus(page, "idle", 8_000);

    await execCmd(page, `[Console]::Write([char]0x1b + ']133;A' + [char]0x07)`);
    await expectTaskStatus(page, "waiting_input", 5_000);

    await execCmd(page, `[Console]::Write([char]0x1b + ']133;C' + [char]0x07)`);
    await expectTaskStatus(page, "running", 5_000);

    // D 之后可能瞬间被 prompt 拉回 running,所以用"曾出现"判定
    await execCmd(page, `[Console]::Write([char]0x1b + ']133;D' + [char]0x07)`);
    let sawIdle = false;
    const start = Date.now();
    while (Date.now() - start < 5_000) {
      if ((await getFirstTaskStatus(page)) === "idle") {
        sawIdle = true;
        break;
      }
      await page.waitForTimeout(100);
    }
    expect(sawIdle).toBe(true);
  });

  test("OSC 633(VSCode 兼容序列)也被识别", async ({ tauri }) => {
    const { page } = tauri;
    await setupTerminal(page);
    await page.waitForTimeout(1_500);
    await expectTaskStatus(page, "idle", 8_000);
    // OSC 633;A 跟 OSC 133;A 等价
    await execCmd(page, `[Console]::Write([char]0x1b + ']633;A' + [char]0x07)`);
    await expectTaskStatus(page, "waiting_input", 5_000);
  });
});
