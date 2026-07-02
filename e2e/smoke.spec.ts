// P0 smoke flows(architecture §I4)
//
// 假定:VibeTerm dev server 已启动在 http://localhost:1420
// 这些测试在 vanilla browser 跑 Web 层(Tauri runtime 由 mock 替代) —
// 主要看 UI 装配、键盘路径、组件可见性。
//
// 完整 Tauri 集成 E2E(driving real Tauri app via tauri-driver)留 M12+。

import { test, expect, type Page } from "@playwright/test";

/** Tauri runtime mock —— 必须 stub event channel,否则 onMount 中第一个 listen() 卡死。
 * 同时给 invoke 一些合理的默认返回,避免组件解构 undefined 报错。 */
async function installTauriMock(page: Page) {
  await page.addInitScript(() => {
    let nextId = 1;
    const fakeTheme = {
      id: "vibe",
      name: "Vibe",
      app: { bg: "#000", text: "#fff", border: "#333", surface: "#111" },
      terminal: { background: "#000", foreground: "#fff", cursor: "#fff" },
      status: { running: "#5a5", waiting: "#fa0", idle: "#888" },
    };
    const defaults: Record<string, unknown> = {
      get_config: { active_theme: "vibe", language: "zh-CN" },
      get_theme: fakeTheme,
      list_tasks: [],
      detect_ai_clis: [
        { name: "claude", installed: false },
        { name: "codex", installed: false },
        { name: "aider", installed: false },
      ],
      list_themes: [fakeTheme],
      // 真实 IPC 返回 KeybindingsFile{bindings},不是裸数组 —— 裸数组会让
      // dispatcher 的 kb.bindings 不可迭代,键盘路径全灭。只 stub 用到的两条。
      get_keybindings: {
        bindings: [
          { command: "command_palette", keys: "Mod+K" },
          { command: "open_settings", keys: "Mod+," },
        ],
      },
      get_env: {},
      get_prompts: [],
    };
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
      invoke: (cmd: string) => {
        if (cmd && cmd.startsWith("plugin:event|")) return Promise.resolve(nextId++);
        if (cmd in defaults) return Promise.resolve(defaults[cmd]);
        return Promise.resolve(null);
      },
      transformCallback: (cb: unknown) => cb,
      // @tauri-apps/api 2.11+ 的 getCurrentWindow/getCurrentWebview 同步读这里,
      // 缺了会在 Titlebar mount 时直接抛 TypeError 整页白屏。
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main", windowLabel: "main" },
      },
    };
  });
}

test.describe("VibeTerm Web Smoke", () => {
  test.beforeEach(async ({ page }) => {
    await installTauriMock(page);
  });

  test("main 窗口加载 + header 显示 VibeTerm", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("strong", { hasText: "VibeTerm" })).toBeVisible({
      timeout: 5000,
    });
  });

  test("Cmd+K 打开命令面板,Esc 关闭", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("strong", { hasText: "VibeTerm" })).toBeVisible();
    const input = page.locator('[data-testid="palette-input"]');
    // header 可见 ≠ 全局 keydown 监听已注册(onMount 里一串 await 之后才 addEventListener),
    // 单次按键会与启动竞态 → 按键+断言整体重试,直到监听就位。
    await expect(async () => {
      await page.keyboard.press("Control+k");
      await expect(input).toBeVisible({ timeout: 300 });
    }).toPass({ timeout: 5000 });
    await expect(input).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(input).toBeHidden({ timeout: 2000 });
  });

  test("Cmd+, 打开设置面板", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("strong", { hasText: "VibeTerm" })).toBeVisible();
    // Settings 组件应该出现 — 用一个能区分的稳定 marker
    // (Settings 顶部有标题 link / 主题选择网格)
    const settingsRoot = page.locator('[data-testid="settings-panel"]');
    // 同上:与全局 keydown 监听注册竞态,按键+断言整体重试
    await expect(async () => {
      await page.keyboard.press("Control+,");
      await expect(settingsRoot).toBeVisible({ timeout: 300 });
    }).toPass({ timeout: 5000 });
  });

  test("空任务态:显示 'create a task' 提示", async ({ page }) => {
    await page.goto("/");
    // empty state 文案(中英文都接受)
    await expect(page.locator("main")).toContainText(/create a task|创建任务|新建任务/i, {
      timeout: 5000,
    });
  });

  test("浮窗 entry 加载 + header 显示任务名", async ({ page }) => {
    await page.goto("/floating.html?taskId=0");
    await expect(page.locator("header")).toBeVisible({ timeout: 5000 });
  });

  test("AI CLI banner:全部未装时出现", async ({ page }) => {
    await page.goto("/");
    // 三个 CLI 全 not installed,banner 应该出现
    // 文案含 "CLI" 或 "AI"(中英日都有)
    const banner = page.locator('[data-testid="cli-banner"]');
    await expect(banner).toBeVisible({ timeout: 3000 });
  });

  // ===== IME / resize 路径(终端)=====
  // 需要一个 spawn 成功的终端:stub list_tasks + spawn_terminal_in_task,
  // 把 write_pty 的字节捕获到 window.__ptyWrites__、resize_pty 捕获到
  // window.__ptyResizes__ 供断言。resize_pty 人为延迟 30ms resolve 并记录
  // in-flight 重叠次数(__resizeOverlaps__):前端串行化正确时恒为 0。
  const installTerminalWithPtyCapture = async (page: Page) => {
    await page.addInitScript(() => {
      const w = window as Window & {
        __TAURI_INTERNALS__?: {
          invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
        };
        __ptyWrites__?: number[][];
        __ptyResizes__?: [number, number][];
        __resizeInflight__?: boolean;
        __resizeOverlaps__?: number;
      };
      w.__ptyWrites__ = [];
      w.__ptyResizes__ = [];
      w.__resizeInflight__ = false;
      w.__resizeOverlaps__ = 0;
      const internals = w.__TAURI_INTERNALS__!;
      const orig = internals.invoke;
      internals.invoke = (cmd: string, args?: Record<string, unknown>) => {
        if (cmd === "list_tasks") {
          return Promise.resolve([
            {
              id: 1,
              name: "ime",
              cwd: null,
              pinned: false,
              status: "idle",
              terminal_ids: [],
              location: { kind: "MainWorkspace" },
              split_tree: { kind: "leaf", slot_id: 0 },
              notify_muted: false,
            },
          ]);
        }
        if (cmd === "spawn_terminal_in_task") {
          return Promise.resolve({ terminal_id: 1, sink_id: null });
        }
        if (cmd === "write_pty") {
          w.__ptyWrites__!.push(args!.data as number[]);
          return Promise.resolve(null);
        }
        if (cmd === "resize_pty") {
          if (w.__resizeInflight__) w.__resizeOverlaps__! += 1;
          w.__resizeInflight__ = true;
          w.__ptyResizes__!.push([args!.rows as number, args!.cols as number]);
          return new Promise((resolve) =>
            setTimeout(() => {
              w.__resizeInflight__ = false;
              resolve(null);
            }, 30),
          );
        }
        return orig(cmd, args);
      };
    });
  };

  const decodedPtyWrites = (page: Page) =>
    page.evaluate(() => {
      const writes = (window as Window & { __ptyWrites__?: number[][] }).__ptyWrites__ ?? [];
      return writes.map((w) => new TextDecoder().decode(Uint8Array.from(w))).join("");
    });

  test("IME:全角标点直提交(keydown 229 + insertText,无 composition)一次到达 PTY", async ({
    page,
  }) => {
    // 回归:macOS 中文输入法 shift+标点(《》?:等)WebKit 只派发 keydown(keyCode=229)
    // + input(insertText),没有 composition 事件。customKeyEventHandler 若无条件拦 229,
    // xterm CompositionHelper._handleAnyTextareaChanges 的 textarea 差分路径被切断,
    // 字符滞留 textarea → 表现为"要连按两次才出一个"。上游同族:xtermjs #3070/#5374。
    await installTerminalWithPtyCapture(page);
    await page.goto("/");
    // data-terminal-id 由 spawn 成功后 setHostAttrs 设置 = onData 已绑定
    await page.locator('[data-terminal-id="1"]').waitFor({ state: "attached", timeout: 10_000 });
    await page.evaluate(() => {
      const ta = document.querySelector(".xterm-helper-textarea") as HTMLTextAreaElement;
      ta.focus();
      // WebKit 序列:IME 吞键 → keydown keyCode=229(isComposing=false) → insertText
      const kd = new KeyboardEvent("keydown", { key: "Process", bubbles: true, cancelable: true });
      Object.defineProperty(kd, "keyCode", { get: () => 229 });
      ta.dispatchEvent(kd);
      ta.value += "《";
      ta.dispatchEvent(
        new InputEvent("input", { data: "《", inputType: "insertText", bubbles: true, composed: true }),
      );
      const ku = new KeyboardEvent("keyup", { key: "Process", bubbles: true, cancelable: true });
      Object.defineProperty(ku, "keyCode", { get: () => 229 });
      ta.dispatchEvent(ku);
    });
    await expect(async () => {
      expect(await decodedPtyWrites(page)).toContain("《");
    }).toPass({ timeout: 3000 });
    // 恰好一次 —— 组件直送与 xterm 原生路径不得双发
    const once = await decodedPtyWrites(page);
    expect(once.split("《").length - 1).toBe(1);
  });

  test("IME:第三方输入法异步 insertText(keydown 后延迟落值)不丢字符", async ({ page }) => {
    // 回归:微信/豆包/搜狗等 IME 对每个键报 keyCode 229,insertText 经输入法进程
    // IPC **异步**落进 textarea,晚于 xterm CompositionHelper 在 keydown 时安排的
    // setTimeout(0) 差分窗口 → 差分扑空,字符滞后一拍,总被下一次按键带出,
    // 表现为"连按两次出一个"(上游同族:xtermjs #5887)。v1.1.4 的差分方案对此无效;
    // 现实现改为 input 事件驱动直送,与落值时机无关。
    await installTerminalWithPtyCapture(page);
    await page.goto("/");
    await page.locator('[data-terminal-id="1"]').waitFor({ state: "attached", timeout: 10_000 });
    await page.evaluate(async () => {
      const ta = document.querySelector(".xterm-helper-textarea") as HTMLTextAreaElement;
      ta.focus();
      const kd = new KeyboardEvent("keydown", { key: "Process", bubbles: true, cancelable: true });
      Object.defineProperty(kd, "keyCode", { get: () => 229 });
      ta.dispatchEvent(kd);
      // 模拟 IME 进程往返:字符在 keydown 同步链与 setTimeout(0) 差分窗口之后才落值
      await new Promise((r) => setTimeout(r, 30));
      ta.value += "！";
      ta.dispatchEvent(
        new InputEvent("input", { data: "！", inputType: "insertText", bubbles: true, composed: true }),
      );
      const ku = new KeyboardEvent("keyup", { key: "Process", bubbles: true, cancelable: true });
      Object.defineProperty(ku, "keyCode", { get: () => 229 });
      ta.dispatchEvent(ku);
    });
    await expect(async () => {
      expect(await decodedPtyWrites(page)).toContain("！");
    }).toPass({ timeout: 3000 });
    const once = await decodedPtyWrites(page);
    expect(once.split("！").length - 1).toBe(1);
  });

  test("IME:composition 期间 Enter 不漏进 PTY,选词文本原子上屏", async ({ page }) => {
    // 守护既有修复:WKWebView 合成期间 Enter/选词键的 keydown 常不带 isComposing/229
    // 标记,靠组件自维护的 composing 标志拦截(claude-code#1547 同症)。
    await installTerminalWithPtyCapture(page);
    await page.goto("/");
    await page.locator('[data-terminal-id="1"]').waitFor({ state: "attached", timeout: 10_000 });
    await page.evaluate(() => {
      const ta = document.querySelector(".xterm-helper-textarea") as HTMLTextAreaElement;
      ta.focus();
      ta.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
      // 合成期间的 Enter:WKWebView 可能不带 isComposing 标记 → 只能靠 composing 标志拦
      const kd = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
      Object.defineProperty(kd, "keyCode", { get: () => 13 });
      ta.dispatchEvent(kd);
      // IME 提交 "你好"
      ta.value += "你好";
      ta.dispatchEvent(new CompositionEvent("compositionend", { data: "你好", bubbles: true }));
      ta.dispatchEvent(
        new InputEvent("input", {
          data: "你好",
          inputType: "insertFromComposition",
          bubbles: true,
          composed: true,
        }),
      );
    });
    await expect(async () => {
      expect(await decodedPtyWrites(page)).toContain("你好");
    }).toPass({ timeout: 3000 });
    expect(await decodedPtyWrites(page)).not.toContain("\r");
  });

  test("PTY resize:聚焦对账触发尺寸断言,且 in-flight 全程串行", async ({ page }) => {
    // 回归:偶发排版错乱(PTY 与 xterm 列数失同步,过去要开关分屏才恢复)。
    // 两道防线:1) focusin 对账 —— 点进终端即 fit + 断言 PTY 尺寸;
    // 2) resize 串行化 —— 背靠靠连发时后端 async 执行顺序无保证,PTY 可能停在
    // 旧尺寸;串行化后任意时刻最多一个 in-flight(mock 延迟 30ms 检测重叠)。
    await installTerminalWithPtyCapture(page);
    await page.goto("/");
    await page.locator('[data-terminal-id="1"]').waitFor({ state: "attached", timeout: 10_000 });
    // 等 mount 期兜底 fit(0/50/200/600/1500ms)消停,再清零计数
    await page.waitForTimeout(1800);
    await page.evaluate(() => {
      const w = window as Window & { __ptyResizes__?: [number, number][]; __resizeOverlaps__?: number };
      w.__ptyResizes__!.length = 0;
      w.__resizeOverlaps__ = 0;
    });
    // 连续两次 focusin → 两次对账背靠背同步连发:无串行化必与 30ms 延迟重叠
    await page.evaluate(() => {
      const host = document.querySelector('[data-terminal-id="1"]')!;
      host.dispatchEvent(new Event("focusin", { bubbles: true }));
      host.dispatchEvent(new Event("focusin", { bubbles: true }));
    });
    // 再走一次真实布局路径:改窗口尺寸 → ResizeObserver → fit → onResize
    await page.setViewportSize({ width: 900, height: 500 });
    await expect(async () => {
      const resizes = await page.evaluate(
        () => (window as Window & { __ptyResizes__?: [number, number][] }).__ptyResizes__!,
      );
      expect(resizes.length).toBeGreaterThan(0);
      // 最终落地尺寸 = xterm 当前尺寸(对账收敛)
      const [rows, cols] = await page.evaluate(() => {
        const host = document.querySelector('[data-terminal-id="1"]') as HTMLElement & {
          __vibeterm_term__: { rows: number; cols: number };
        };
        return [host.__vibeterm_term__.rows, host.__vibeterm_term__.cols];
      });
      expect(resizes[resizes.length - 1]).toEqual([rows, cols]);
    }).toPass({ timeout: 5000 });
    const overlaps = await page.evaluate(
      () => (window as Window & { __resizeOverlaps__?: number }).__resizeOverlaps__!,
    );
    expect(overlaps).toBe(0);
  });

  test("任务右键菜单:Portal 到 body 且整体留在视口内", async ({ page }) => {
    // 回归:菜单曾是裸 fixed 定位 — 贴边右键伸出窗口被裁切,canvas 模式还会陷进
    // transform/stacking context 被遮挡。修复 = Portal 到 body + 实测尺寸视口夹取。
    await page.addInitScript(() => {
      const internals = (window as Window & {
        __TAURI_INTERNALS__?: { invoke: (cmd: string) => Promise<unknown> };
      }).__TAURI_INTERNALS__!;
      const orig = internals.invoke;
      internals.invoke = (cmd: string) => {
        if (cmd === "list_tasks") {
          return Promise.resolve([
            {
              id: 1,
              name: "demo",
              cwd: null,
              pinned: false,
              status: "idle",
              terminal_ids: [],
              location: { kind: "MainWorkspace" },
              split_tree: { kind: "leaf", slot_id: 0 },
              notify_muted: false,
            },
          ]);
        }
        return orig(cmd);
      };
    });
    // 故意压扁窗口:任务行下方放不下菜单,夹取逻辑必须把菜单收回视口
    await page.setViewportSize({ width: 600, height: 220 });
    await page.goto("/");
    const row = page.locator(".task-row");
    await expect(row).toBeVisible({ timeout: 5000 });
    await row.click({ button: "right" });
    const menu = page.locator('[data-testid="task-ctx-menu"]');
    await expect(menu).toBeVisible({ timeout: 2000 });
    // Portal 生效:菜单已逃出侧栏子树(Solid Portal 在 body 下挂容器 div)
    const escaped = await menu.evaluate(
      (el) => !el.closest('[data-testid="task-list"]') && el.parentElement?.parentElement === document.body,
    );
    expect(escaped).toBe(true);
    // 整体留在视口内(含 8px margin 的余量判断放宽到 0/视口边)
    const box = (await menu.boundingBox())!;
    const vp = page.viewportSize()!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(vp.width);
    expect(box.y + box.height).toBeLessThanOrEqual(vp.height);
  });
});
