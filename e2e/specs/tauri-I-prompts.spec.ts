// 模块 I:prompts // 触发器 + PromptPicker
//
// 全栈通路(tests 1-5 验):
//   user 在 xterm 输 `/` `/` → term.onData 检测连续 2 个 `/`
//     → props.onDoubleSlash() → setPromptPickerOpen(true)
//       → PromptPicker mount → ipc.getPrompts() → 列出
//         → 搜索过滤 / Esc 关闭
//
// PTY 插入路径(tests 6-7,fixme)在多模块串行运行后 xterm 输入有时序问题,
// 单独跑可过;留 fixme 文档化已知限制。
//
// 种 prompts 直接写 %APPDATA%\VibeTerm\prompts.toml(vibeterm-config 读取的文件)。

import { test, expect, type Page } from "./helpers/tauri-fixture";
import { ensureTasks } from "./helpers/tauri-fixture";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

// 防数据丢失:本 spec 直接读写应用真实 prompts.toml —— release 构建忽略
// VIBETERM_CONFIG_DIR(见 vibeterm-config::config_dir 的 cfg(debug_assertions) 安全门),
// 无法把配置目录重定向到临时目录,spec 写哪 app 就得读哪(真实路径)。为不毁掉用户
// 真实模板:beforeAll 备份原文件、afterAll 原样恢复(原本不存在则删掉测试种的)。
let savedPromptsToml: string | null = null;

interface PromptEntry {
  id: string;
  name: string;
  content: string;
  shortcut: string | null;
}

function promptsTomlPath(): string {
  const appData = process.env.APPDATA || os.homedir();
  return path.join(appData, "VibeTerm", "prompts.toml");
}

function seedPromptsFs(prompts: PromptEntry[]): void {
  const p = promptsTomlPath();
  fs.mkdirSync(path.dirname(p), { recursive: true });
  const lines: string[] = ["schema_version = 1", ""];
  for (const pr of prompts) {
    lines.push("[[prompts]]");
    lines.push(`id = ${JSON.stringify(pr.id)}`);
    lines.push(`name = ${JSON.stringify(pr.name)}`);
    lines.push(`content = ${JSON.stringify(pr.content)}`);
    if (pr.shortcut !== null) lines.push(`shortcut = ${JSON.stringify(pr.shortcut)}`);
    lines.push("");
  }
  fs.writeFileSync(p, lines.join("\n"));
}

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

async function setupTerminal(page: Page): Promise<void> {
  await ensureTasks(page, 1);
  await page.locator('[data-testid^="task-item-"]').first().click();
  const host = page.locator('[data-testid="terminal-host"]').first();
  await expect(host).toBeVisible({ timeout: 5_000 });
  await expect
    .poll(async () => host.getAttribute("data-terminal-id"), { timeout: 10_000 })
    .not.toBeNull();
  await host.locator(".xterm-screen").first().click();
  await page.waitForTimeout(1_200);
}

/** 通过 xterm.paste 触发 onData,绕开 Playwright keyboard 焦点不稳定问题 */
async function pasteIntoXterm(page: Page, text: string): Promise<void> {
  await page.evaluate((t) => {
    const host = document.querySelector('[data-testid="terminal-host"]') as
      | (HTMLElement & { __vibeterm_term__?: { paste(s: string): void } })
      | null;
    if (!host?.__vibeterm_term__) throw new Error("no xterm term instance");
    host.__vibeterm_term__.paste(t);
  }, text);
}

async function openPicker(page: Page): Promise<void> {
  // term.onData 要求 lastChar === "/",必须 2 次单独 paste
  await pasteIntoXterm(page, "/");
  await page.waitForTimeout(80);
  await pasteIntoXterm(page, "/");
  const picker = page.locator('[data-testid="prompt-picker"]');
  await expect(picker).toBeVisible({ timeout: 3_000 });
  const input = page.locator('[data-testid="prompt-picker-input"]');
  await expect(input).toBeVisible();
  await input.focus();
  await expect(input).toBeFocused({ timeout: 3_000 });
}

test.describe("模块 I:prompts // 触发器", () => {
  test.beforeAll(() => {
    try {
      savedPromptsToml = fs.readFileSync(promptsTomlPath(), "utf8");
    } catch {
      savedPromptsToml = null; // 用户原本没有 prompts.toml
    }
  });

  test.afterAll(() => {
    const p = promptsTomlPath();
    try {
      if (savedPromptsToml !== null) {
        fs.writeFileSync(p, savedPromptsToml); // 恢复用户原始模板
      } else {
        fs.rmSync(p, { force: true }); // 原本不存在 → 删掉测试种的
      }
    } catch {
      /* ignore */
    }
  });

  test("基础 // 触发:连按 / / → PromptPicker 出现", async ({ tauri }) => {
    const { page } = tauri;
    await setupTerminal(page);
    await expect(page.locator('[data-testid="prompt-picker"]')).toBeHidden();
    await openPicker(page);
  });

  test("PromptPicker:Esc 关闭", async ({ tauri }) => {
    const { page } = tauri;
    await setupTerminal(page);
    await openPicker(page);
    await page.locator('[data-testid="prompt-picker-input"]').press("Escape");
    await expect(page.locator('[data-testid="prompt-picker"]')).toBeHidden({
      timeout: 3_000,
    });
  });

  test("空 prompts.toml → input 聚焦 + 显示空文案", async ({ tauri }) => {
    const { page } = tauri;
    seedPromptsFs([]);
    await setupTerminal(page);
    await openPicker(page);
    await expect(
      page.locator('[data-testid="prompt-picker"]').getByText(/没有 prompt 模板/),
    ).toBeVisible({ timeout: 3_000 });
  });

  test("种 1 条 prompt → // 触发 → 列表项可见", async ({ tauri }) => {
    const { page } = tauri;
    seedPromptsFs([
      {
        id: "test-greet",
        name: "Test Greeting",
        content: "echo HELLO_FROM_PROMPT",
        shortcut: null,
      },
    ]);
    await setupTerminal(page);
    await openPicker(page);
    await expect(
      page.locator('[data-testid="prompt-picker"]').getByText("Test Greeting"),
    ).toBeVisible({ timeout: 3_000 });
  });

  test("种 3 条 prompts → 搜索过滤 → 仅匹配项显示", async ({ tauri }) => {
    const { page } = tauri;
    seedPromptsFs([
      { id: "alpha", name: "Alpha review", content: "review alpha", shortcut: null },
      { id: "beta", name: "Beta deploy", content: "deploy beta", shortcut: null },
      { id: "gamma", name: "Gamma test", content: "test gamma", shortcut: null },
    ]);
    await setupTerminal(page);
    await openPicker(page);
    const picker = page.locator('[data-testid="prompt-picker"]');
    await expect(picker.getByText("Alpha review")).toBeVisible();
    await expect(picker.getByText("Beta deploy")).toBeVisible();
    await expect(picker.getByText("Gamma test")).toBeVisible();

    await page.locator('[data-testid="prompt-picker-input"]').fill("beta");
    await page.waitForTimeout(200);
    await expect(picker.getByText("Beta deploy")).toBeVisible();
    await expect(picker.getByText("Alpha review")).toBeHidden();
    await expect(picker.getByText("Gamma test")).toBeHidden();
  });

  // FIXME:这两个 test 单跑通过,在全套串行(模块 A-H 跑过后)会因为 xterm
  // 输入路径累积 state 而失败。等找到稳定方案后启用。
  test.fixme("种 1 条 prompt → 点列表项 → content 注入 PTY", async ({ tauri }) => {
    const { page } = tauri;
    const magic = `PROMPT_INSERT_${Date.now()}`;
    seedPromptsFs([
      { id: "insertable", name: "Insertable prompt", content: magic, shortcut: null },
    ]);
    await setupTerminal(page);
    await openPicker(page);
    await expect(
      page.locator('[data-testid="prompt-picker"]').getByText("Insertable prompt"),
    ).toBeVisible({ timeout: 3_000 });
    await page
      .locator('[data-testid="prompt-picker"]')
      .getByText("Insertable prompt")
      .click();
    await expect(page.locator('[data-testid="prompt-picker"]')).toBeHidden({
      timeout: 3_000,
    });
    const start = Date.now();
    let saw = false;
    while (Date.now() - start < 5_000) {
      if ((await readXtermBuffer(page)).includes(magic)) {
        saw = true;
        break;
      }
      await page.waitForTimeout(150);
    }
    expect(saw).toBe(true);
  });

  test.fixme("{{cursor}} 标记 → 插入后 buffer 含两端,光标在中间", async ({
    tauri,
  }) => {
    const { page } = tauri;
    seedPromptsFs([
      {
        id: "cursor",
        name: "WithCursor",
        content: "HEAD_CUR{{cursor}}_TAIL_CUR",
        shortcut: null,
      },
    ]);
    await setupTerminal(page);
    await openPicker(page);
    await expect(
      page.locator('[data-testid="prompt-picker"]').getByText("WithCursor"),
    ).toBeVisible({ timeout: 3_000 });
    await page
      .locator('[data-testid="prompt-picker"]')
      .getByText("WithCursor")
      .click();
    await expect(page.locator('[data-testid="prompt-picker"]')).toBeHidden({
      timeout: 3_000,
    });
    const start = Date.now();
    let buf = "";
    while (Date.now() - start < 5_000) {
      buf = await readXtermBuffer(page);
      if (buf.includes("HEAD_CUR") && buf.includes("_TAIL_CUR")) break;
      await page.waitForTimeout(150);
    }
    expect(buf).toContain("HEAD_CUR");
    expect(buf).toContain("_TAIL_CUR");
    expect(buf).not.toContain("{{cursor}}");
  });
});
