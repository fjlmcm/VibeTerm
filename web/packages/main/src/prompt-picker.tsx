// Prompt 选择器
//
// 触发方式: 双击 Cmd (默认) / 用户自定义 keybinding.
// 区分两类 prompt:
//   - kind="agent" — 当前 task 跑 agent (claude / codex / aider 等) 时显示
//   - kind="terminal" — 当前 task 在普通 shell 时显示
//   - kind 未指定 → 全部显示
// 焦点: 显式 ref + onMount focus, 同时 window-level keydown 兜底 (input 失焦也能用).

import { For, Show, createMemo, createSignal, onMount, onCleanup, type Component } from "solid-js";
import { ipc, t, focusTerminal, promptDisplayName, promptDisplayContent } from "@vibeterm/ui-core";
import type { PromptEntry, PromptKind, TerminalId } from "@vibeterm/ipc-types";

// {{cursor}} 回退用的终端显示列宽 — ESC[nD (CUB) 按"列"左移光标, 不是按 UTF-16
// code unit. CJK / 全角 / Emoji 在终端 (xterm Unicode 15 graphemes) 占 2 列, 但 JS
// .length 对 BMP CJK 只算 1, 对非 BMP emoji 算 2. 用 for...of 按 code point 迭代,
// 宽字符算 2 列、组合记号算 0 列, 其余算 1 列, 与 xterm 渲染宽度对齐.
function displayWidth(s: string): number {
  let width = 0;
  for (const ch of s) {
    width += charWidth(ch.codePointAt(0) ?? 0);
  }
  return width;
}

function charWidth(cp: number): number {
  // 组合记号 (无独立列宽)
  if (
    (cp >= 0x0300 && cp <= 0x036f) || // 组合变音符
    (cp >= 0x1ab0 && cp <= 0x1aff) ||
    (cp >= 0x1dc0 && cp <= 0x1dff) ||
    (cp >= 0x20d0 && cp <= 0x20ff) ||
    (cp >= 0xfe20 && cp <= 0xfe2f)
  ) {
    return 0;
  }
  // 东亚宽 / 全角 / Emoji — 终端占 2 列
  if (
    (cp >= 0x1100 && cp <= 0x115f) || // 韩文字母 Jamo
    (cp >= 0x2e80 && cp <= 0x303e) || // CJK 部首补充 .. CJK 符号
    (cp >= 0x3041 && cp <= 0x33ff) || // 平假名 .. CJK 兼容
    (cp >= 0x3400 && cp <= 0x4dbf) || // CJK 扩展 A
    (cp >= 0x4e00 && cp <= 0x9fff) || // CJK 统一表意文字
    (cp >= 0xa000 && cp <= 0xa4cf) || // 彝文
    (cp >= 0xac00 && cp <= 0xd7a3) || // 韩文音节
    (cp >= 0xf900 && cp <= 0xfaff) || // CJK 兼容表意文字
    (cp >= 0xfe30 && cp <= 0xfe4f) || // CJK 兼容形式
    (cp >= 0xff00 && cp <= 0xff60) || // 全角 ASCII
    (cp >= 0xffe0 && cp <= 0xffe6) || // 全角符号
    (cp >= 0x1f300 && cp <= 0x1faff) || // Emoji / 符号扩展
    (cp >= 0x20000 && cp <= 0x3fffd) // CJK 扩展 B+
  ) {
    return 2;
  }
  return 1;
}

export interface PromptPickerProps {
  terminalId: TerminalId | null;
  onClose: () => void;
  /** 根据 kind 过滤 prompts. undefined = 全部. */
  kind?: PromptKind;
}

export const PromptPicker: Component<PromptPickerProps> = (props) => {
  const [prompts, setPrompts] = createSignal<PromptEntry[]>([]);
  const [q, setQ] = createSignal("");
  // highlighted=-1 表示用户尚未主动选择. 防止 picker 误弹出时 Enter 误 insert.
  // 用户按 ArrowDown / 开始输入搜索 → 设为 0 才进入"可 Enter 选中"状态.
  const [highlighted, setHighlighted] = createSignal(-1);
  // kind 用 signal — props.kind 是 task DTO 的 agent_kind (3s 缓存),
  // mount 时立即对当前焦点 terminal 做一次实时嗅探, 把结果覆盖到 detectedKind.
  // 用户焦点是更准的信号 — 他在哪个 terminal 双击 Cmd, 就嗅探哪个.
  const [detectedKind, setDetectedKind] = createSignal<PromptKind | undefined>(props.kind);
  const effectiveKind = () => detectedKind() ?? props.kind;
  let inputEl: HTMLInputElement | undefined;
  let listEl: HTMLDivElement | undefined;

  onMount(async () => {
    // 立即对焦点所在 terminal 做实时 agent 嗅探, 覆盖 props.kind.
    // 即使后台 3s 轮询还没识别到 codex, 用户在 codex 终端双击 Cmd 也能立刻拿到正确 kind.
    if (props.terminalId !== null) {
      ipc
        .detectAgentForTerminal(props.terminalId)
        .then((r) => {
          setDetectedKind(r.agent_kind ? "agent" : "terminal");
        })
        .catch((e) => console.warn("[prompt-picker] detect agent failed", e));
    } else {
      console.warn("[prompt-picker] no terminalId, cannot detect");
    }
    try {
      const f = await ipc.getPrompts();
      setPrompts(f.prompts);
    } catch (e) {
      console.error("[prompt-picker] load failed", e);
    }
    requestAnimationFrame(() => {
      inputEl?.focus();
      inputEl?.select();
    });
  });

  const filtered = createMemo(() => {
    const query = q().trim().toLowerCase();
    const kind = effectiveKind();
    return prompts()
      .filter((p) => {
        const k = p.kind ?? "agent";
        if (kind && k !== kind) return false;
        if (!query) return true;
        // i18n 显示名 + 显示内容 + id + 原 name + 原 content 都参与匹配
        const display = promptDisplayName(p).toLowerCase();
        const displayContent = promptDisplayContent(p).toLowerCase();
        return (
          display.includes(query) ||
          displayContent.includes(query) ||
          p.id.toLowerCase().includes(query) ||
          p.name.toLowerCase().includes(query) ||
          p.content.toLowerCase().includes(query)
        );
      });
  });

  const insert = async (p: PromptEntry) => {
    if (props.terminalId === null) {
      props.onClose();
      return;
    }
    // 替换当前行 — 选 prompt 模板 = 得到完整一条命令, 不跟用户已输字符拼接.
    // zsh/bash 的 Ctrl+U (\x15) = "删光标到行首"; Ink-based agent TUI (claude/codex/
    // aider) 的 readline 也识别此序列, 行为一致.
    const ctrlU = new Uint8Array([0x15]);
    await ipc.writePty(props.terminalId, ctrlU).catch(console.error);

    // {{cursor}} 光标定位 — 内容用 displayContent (按当前 lang 翻译, fallback p.content)
    const rawContent = promptDisplayContent(p);
    const marker = "{{cursor}}";
    const idx = rawContent.indexOf(marker);
    const clean = rawContent.replace(/\{\{cursor\}\}/g, "");
    await ipc
      .writePty(props.terminalId, new TextEncoder().encode(clean))
      .catch(console.error);

    if (idx >= 0) {
      // {{cursor}} 右侧内容的终端列宽 — 不能用 .length (CJK/Emoji 列宽 != code unit 数).
      const backCount = displayWidth(clean.slice(idx));
      if (backCount > 0) {
        const esc = `\x1b[${backCount}D`;
        await ipc
          .writePty(props.terminalId, new TextEncoder().encode(esc))
          .catch(console.error);
      }
    }
    // 让 xterm 拿回焦点, 用户不必点击就能直接按 Enter 提交.
    const tid = props.terminalId;
    props.onClose();
    requestAnimationFrame(() => focusTerminal(tid));
  };

  // 防重复:Enter 按下 → handleKey 同时被 window capture + input target/bubble 触发
  // 会导致 insert N 次. 用一个 generation 锁: handleKey 在 capture 命中后置 true,
  // target/bubble 阶段看到 true 直接 return.
  let consumedThisKey = false;

  const handleKey = (e: KeyboardEvent): boolean => {
    if (e.isComposing || e.keyCode === 229) return false;
    if (consumedThisKey) return true;
    const items = filtered();
    let consumed = false;
    if (e.key === "ArrowDown") {
      setHighlighted((h) => Math.min(items.length - 1, Math.max(0, h) + (h < 0 ? 0 : 1)));
      scrollHighlightedIntoView();
      consumed = true;
    } else if (e.key === "ArrowUp") {
      setHighlighted((h) => Math.max(0, h - 1));
      scrollHighlightedIntoView();
      consumed = true;
    } else if (e.key === "Enter") {
      // 必须有主动选中 (highlighted >= 0) 才 insert; 防误弹场景下 Enter 误触
      const h = highlighted();
      if (h >= 0) {
        const it = items[h];
        if (it) insert(it);
      } else {
        // 没主动选 → 关闭, 不 insert (用户透传 Enter 给 PTY 由后续按键完成)
        props.onClose();
      }
      consumed = true;
    } else if (e.key === "Escape") {
      props.onClose();
      consumed = true;
    }
    if (consumed) {
      e.preventDefault();
      e.stopPropagation();
      e.stopImmediatePropagation();
      consumedThisKey = true;
      // 下一帧重置,允许后续按键再次处理
      queueMicrotask(() => {
        consumedThisKey = false;
      });
    }
    return consumed;
  };

  const onWindowKey = (e: KeyboardEvent) => {
    handleKey(e);
  };

  const onKey = (e: KeyboardEvent) => handleKey(e);

  // 关键:listener 注册 + onCleanup 必须在组件函数体顶层同步执行 (不能放进 onMount 的
  // async function — await 之后调 onCleanup 已脱离 SolidJS owner scope, 注册无效).
  // 否则:组件 unmount 时 listener 不卸载 → 每打开一次 picker leak 一个全局 keydown
  // listener → 用户后续按 Enter 触发 leaked listener → 自动 insert 上次 highlighted
  // 的 prompt. 这是 "用户输 codex 没主动开 picker 却被插入 grep -rn ..." 的真根因.
  window.addEventListener("keydown", onWindowKey, true);
  onCleanup(() => window.removeEventListener("keydown", onWindowKey, true));

  const scrollHighlightedIntoView = () => {
    if (!listEl) return;
    const child = listEl.children[highlighted()] as HTMLElement | undefined;
    child?.scrollIntoView({ block: "nearest" });
  };

  const placeholderKey = () => {
    const k = effectiveKind();
    if (k === "terminal") return "prompts.placeholder.terminal";
    if (k === "agent") return "prompts.placeholder.agent";
    return "prompts.placeholder.all";
  };

  return (
    <div
      data-testid="prompt-picker"
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.3)",
        display: "flex",
        "justify-content": "center",
        "align-items": "flex-end",
        "padding-bottom": "20vh",
        "z-index": 1500,
      }}
      onClick={props.onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--color-surface)",
          border: "1px solid var(--color-border)",
          "border-radius": "8px",
          width: "500px",
          "max-width": "90vw",
          "max-height": "40vh",
          display: "flex",
          "flex-direction": "column",
          overflow: "hidden",
          "box-shadow": "0 8px 24px rgba(0,0,0,0.4)",
        }}
      >
        <input
          ref={(el) => (inputEl = el)}
          data-testid="prompt-picker-input"
          value={q()}
          onInput={(e) => {
            setQ(e.currentTarget.value);
            // 用户开始输入搜索 → 默认选中第一条; 空字符串保持 -1
            setHighlighted(e.currentTarget.value.length > 0 ? 0 : -1);
          }}
          onKeyDown={onKey}
          placeholder={t(placeholderKey())}
          style={{
            background: "var(--color-bg)",
            color: "var(--color-text)",
            border: "none",
            "border-bottom": "1px solid var(--color-border)",
            padding: "10px 14px",
            "font-size": "13px",
            outline: "none",
          }}
        />
        <div ref={(el) => (listEl = el)} style={{ "overflow-y": "auto", flex: "1" }}>
          <For each={filtered()}>
            {(p, i) => (
              <div
                onClick={() => insert(p)}
                onMouseEnter={() => setHighlighted(i())}
                style={{
                  padding: "8px 14px",
                  cursor: "pointer",
                  background: i() === highlighted() ? "var(--color-accent-subtle)" : "transparent",
                  color: "var(--color-text)",
                  "font-size": "12px",
                  "border-bottom": "1px solid var(--color-border)",
                }}
              >
                <div style={{ display: "flex", "align-items": "center", gap: "8px" }}>
                  <span style={{ "font-weight": 600 }}>{promptDisplayName(p)}</span>
                  <span style={{
                    "font-size": "9px",
                    color: "var(--color-text-2)",
                    "padding": "1px 4px",
                    border: "1px solid var(--color-border)",
                    "border-radius": "3px",
                  }}>
                    {p.kind === "terminal" ? "⌨" : "✨"}
                  </span>
                </div>
                <div style={{ color: "var(--color-text-2)", "font-size": "10px", "margin-top": "2px", "white-space": "nowrap", overflow: "hidden", "text-overflow": "ellipsis" }}>
                  {promptDisplayContent(p).replace(/\n/g, " ").slice(0, 80)}
                </div>
              </div>
            )}
          </For>
          <Show when={filtered().length === 0}>
            <div style={{ padding: "16px", color: "var(--color-text-2)", "font-size": "12px", "text-align": "center" }}>
              {t("prompts.empty")}
            </div>
          </Show>
        </div>
      </div>
    </div>
  );
};

