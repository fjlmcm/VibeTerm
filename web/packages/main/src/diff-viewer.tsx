// Diff 查看器 —— 借鉴 cmux diff-viewer 的三源(unstaged / staged / vs base)。
//
// 🟢 零侵入:纯只读 `git diff`(后端 git_diff IPC),不碰 agent 配置、不做 agent-turn 追踪。
// 解析 unified diff → 文件 / hunk / 行,+/- 着色。超大 diff 后端已限额截断。
import {
  type Component,
  For,
  Show,
  createResource,
  createSignal,
  onCleanup,
  onMount,
} from "solid-js";
import { ipc, t } from "@vibeterm/ui-core";

type Source = "unstaged" | "staged" | "base";

interface DiffLine {
  kind: "add" | "del" | "ctx" | "meta";
  text: string;
}
interface DiffFile {
  path: string;
  lines: DiffLine[];
}

// 把 unified diff 原文解析为 文件 → 行。简单稳健,不依赖外部库。
function parseUnifiedDiff(raw: string): DiffFile[] {
  const files: DiffFile[] = [];
  let cur: DiffFile | null = null;
  for (const line of raw.split("\n")) {
    if (line.startsWith("diff --git ")) {
      // 形如 "diff --git a/src/foo.rs b/src/foo.rs"
      const m = line.match(/ b\/(.+)$/);
      cur = { path: m ? m[1] : line.slice("diff --git ".length), lines: [] };
      files.push(cur);
      continue;
    }
    if (!cur) continue;
    if (
      line.startsWith("index ") ||
      line.startsWith("--- ") ||
      line.startsWith("+++ ") ||
      line.startsWith("new file") ||
      line.startsWith("deleted file") ||
      line.startsWith("rename ") ||
      line.startsWith("similarity ") ||
      line.startsWith("@@")
    ) {
      cur.lines.push({ kind: "meta", text: line });
    } else if (line.startsWith("+")) {
      cur.lines.push({ kind: "add", text: line });
    } else if (line.startsWith("-")) {
      cur.lines.push({ kind: "del", text: line });
    } else if (line.startsWith("\\")) {
      cur.lines.push({ kind: "meta", text: line });
    } else {
      cur.lines.push({ kind: "ctx", text: line });
    }
  }
  return files;
}

const lineBg = (kind: DiffLine["kind"]): string => {
  switch (kind) {
    case "add":
      return "color-mix(in oklch, var(--color-status-running, #3b82f6) 16%, transparent)";
    case "del":
      return "color-mix(in oklch, var(--color-status-waiting, #e5a23d) 16%, transparent)";
    case "meta":
      return "var(--color-surface)";
    default:
      return "transparent";
  }
};
const lineFg = (kind: DiffLine["kind"]): string =>
  kind === "meta" ? "var(--color-text-2)" : "var(--color-text)";

export interface DiffViewerProps {
  cwd: string;
  onClose: () => void;
}

export const DiffViewer: Component<DiffViewerProps> = (props) => {
  const [source, setSource] = createSignal<Source>("unstaged");

  const [data] = createResource(source, async (s) => {
    try {
      return await ipc.gitDiff(props.cwd, s);
    } catch (e) {
      console.error("[diff] gitDiff failed", e);
      return null;
    }
  });

  const files = () => {
    const d = data();
    return d ? parseUnifiedDiff(d.raw) : [];
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    }
  };
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => window.removeEventListener("keydown", onKey));

  const tab = (s: Source, label: string) => (
    <button
      data-testid={`diff-tab-${s}`}
      onClick={() => setSource(s)}
      style={{
        padding: "6px 14px",
        "font-size": "13px",
        "font-weight": source() === s ? "600" : "400",
        background: source() === s ? "var(--color-accent-subtle)" : "transparent",
        color: source() === s ? "var(--color-text)" : "var(--color-text-2)",
        border: "none",
        "border-bottom": source() === s ? "2px solid var(--color-accent)" : "2px solid transparent",
        cursor: "pointer",
      }}
    >
      {label}
    </button>
  );

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.4)",
        display: "flex",
        "justify-content": "center",
        "align-items": "flex-start",
        "padding-top": "6vh",
        "z-index": 2100,
      }}
      onClick={props.onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--color-bg)",
          border: "1px solid var(--color-border)",
          "border-radius": "10px",
          width: "920px",
          "max-width": "94vw",
          "max-height": "82vh",
          display: "flex",
          "flex-direction": "column",
          overflow: "hidden",
          "box-shadow": "0 12px 32px rgba(0,0,0,0.5)",
        }}
      >
        {/* 头:标题 + 三源 tab */}
        <div
          style={{
            display: "flex",
            "align-items": "center",
            gap: "4px",
            padding: "8px 12px",
            "border-bottom": "1px solid var(--color-border)",
          }}
        >
          <span style={{ "font-size": "13px", "font-weight": "600", color: "var(--color-text)", "margin-right": "8px" }}>
            {t("diff.title")}
          </span>
          {tab("unstaged", t("diff.tab.unstaged"))}
          {tab("staged", t("diff.tab.staged"))}
          {tab("base", t("diff.tab.base"))}
          <Show when={source() === "base" && data()?.base}>
            <span style={{ "font-size": "11px", color: "var(--color-text-2)", "font-family": "monospace", "margin-left": "4px" }}>
              {data()?.base}
            </span>
          </Show>
          <span style={{ flex: "1" }} />
          <button
            data-testid="diff-close"
            onClick={props.onClose}
            style={{ background: "transparent", border: "none", color: "var(--color-text-2)", cursor: "pointer", "font-size": "16px", padding: "2px 8px" }}
          >
            ✕
          </button>
        </div>

        {/* 体 */}
        <div style={{ flex: "1", overflow: "auto", "font-family": "var(--font-mono, monospace)", "font-size": "12px" }}>
          <Show
            when={!data.loading}
            fallback={<div style={{ padding: "20px", color: "var(--color-text-2)" }}>{t("diff.loading")}</div>}
          >
            <Show
              when={data()}
              fallback={<div style={{ padding: "20px", color: "var(--color-status-waiting, #e5a23d)" }}>{t("diff.error")}</div>}
            >
              <Show
                when={files().length > 0}
                fallback={<div style={{ padding: "20px", color: "var(--color-text-2)" }}>{t("diff.empty")}</div>}
              >
                <For each={files()}>
                  {(f) => (
                    <div>
                      <div
                        style={{
                          position: "sticky",
                          top: 0,
                          background: "var(--color-surface)",
                          color: "var(--color-text)",
                          padding: "6px 12px",
                          "font-weight": "600",
                          "border-top": "1px solid var(--color-border)",
                          "border-bottom": "1px solid var(--color-border)",
                        }}
                      >
                        {f.path}
                      </div>
                      <For each={f.lines}>
                        {(ln) => (
                          <div
                            style={{
                              "white-space": "pre-wrap",
                              "word-break": "break-all",
                              padding: "0 12px",
                              background: lineBg(ln.kind),
                              color: lineFg(ln.kind),
                            }}
                          >
                            {ln.text || " "}
                          </div>
                        )}
                      </For>
                    </div>
                  )}
                </For>
                <Show when={data()?.truncated}>
                  <div style={{ padding: "12px", color: "var(--color-status-waiting, #e5a23d)", "font-size": "12px" }}>
                    {t("diff.truncated")}
                  </div>
                </Show>
              </Show>
            </Show>
          </Show>
        </div>
      </div>
    </div>
  );
};
