// 模块 J:启动时序回归测试
//
// 历史 bug:用户打开 app 时,如果 tasks.toml 有持久任务,会自动 mount
// Terminal,但 Tauri Channel bridge 还没就绪,shell 启动产生的 PowerShell
// banner / prompt 输出被 channel.send 静默丢弃 → 主区域永远空白。
//
// 修复:terminal/index.tsx 在 requestAnimationFrame 里加 50ms 微延迟,
//      让 Tauri runtime 完成内部初始化后再 new Channel + spawn。
//
// 这个 test 必须用 ensureTasks + 启动后立即检查 buffer(不能用 createFreshTask
// 因为后者走 user-click flow,bridge 早已暖)。

import { test, expect, type Page } from "./helpers/tauri-fixture";
import { ensureTasks } from "./helpers/tauri-fixture";

async function readBuffer(page: Page): Promise<{ chars: number; firstLine: string }> {
  return page.evaluate(() => {
    const host = document.querySelector('[data-testid="terminal-host"]');
    const term = host?.__vibeterm_term__;
    if (!term) return { chars: 0, firstLine: "" };
    const lines: string[] = [];
    for (let i = 0; i < term.buffer.active.length; i++) {
      const l = term.buffer.active.getLine(i);
      if (l) lines.push(l.translateToString(true));
    }
    return {
      chars: lines.join("").length,
      firstLine: lines.find((l) => l.trim().length) ?? "",
    };
  });
}

test.describe("模块 J:启动时序回归", () => {
  test("auto-mounted Terminal(初始活跃任务)— shell prompt 在 5s 内出现", async ({
    tauri,
  }) => {
    const { page } = tauri;
    // 模拟用户场景:确保至少有 1 个任务(若 tasks.toml 已有就用,否则创建)
    await ensureTasks(page, 1);
    // 等 onMount 自动激活第 1 个任务 + Terminal 自动 mount + shell 启动 + prompt 渲染
    await expect
      .poll(async () => (await readBuffer(page)).chars, { timeout: 10_000 })
      .toBeGreaterThan(0);
    const buf = await readBuffer(page);
    expect(buf.chars).toBeGreaterThan(0);
    // PowerShell / cmd.exe / pwsh 都会写至少一行 prompt 或 banner
    expect(buf.firstLine.length).toBeGreaterThan(0);
  });
});
