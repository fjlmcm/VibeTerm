// 自定义标题栏(替代 Windows / macOS 原生)
//
// - drag region 用 data-tauri-drag-region(Tauri 2 原生支持)
// - min / max / close 按钮调 WebviewWindow.current()
// - 颜色用主题变量,跟应用一致
// - macOS 上 traffic lights 已在左上(决定下次再做)

import { type Component, type JSX, createSignal, onCleanup, onMount } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-solid";
import { t } from "../i18n";

export interface TitlebarProps {
  /** 标题(中间区,可选 — 留空则不渲染文字)*/
  title?: string;
  /** 左侧区(放 app 名 / breadcrumb 等)*/
  left?: JSX.Element;
  /** 中间附加(放 banner / status 等)*/
  center?: JSX.Element;
  /** 右侧 action 区(主窗的 settings 按钮等);macOS 上紧贴右边,Win 上排在 min/max/close 左侧 */
  right?: JSX.Element;
}

// macOS 用原生 titleBar Overlay(traffic lights 由系统画在左上),
// 我们的自渲按钮 + 左侧 padding 都按平台分支
const IS_MAC =
  typeof navigator !== "undefined" && /Mac|iPhone|iPod|iPad/.test(navigator.userAgent);

export const Titlebar: Component<TitlebarProps> = (props) => {
  const win = getCurrentWindow();
  const [maximized, setMaximized] = createSignal(false);

  const sync = async () => {
    try {
      setMaximized(await win.isMaximized());
    } catch {
      /* ignore */
    }
  };

  onMount(async () => {
    await sync();
    try {
      const unlisten = await win.onResized(() => sync());
      onCleanup(() => {
        if (typeof unlisten === "function") unlisten();
      });
    } catch {
      /* mock 或 webview 未就绪时忽略 */
    }
  });

  const onMinimize = () => win.minimize().catch(() => {});
  const onMaximize = async () => {
    if (await win.isMaximized()) await win.unmaximize().catch(() => {});
    else await win.maximize().catch(() => {});
    sync();
  };
  const onClose = () => win.close().catch(() => {});

  // `data-tauri-drag-region` 只看 mousedown target 自身的 dataset, 不冒泡 ->
  // 点中子元素文字时不会触发. 用 onMouseDown 主动调 startDragging API,
  // 排除交互控件即可覆盖整条 titlebar.
  const onDragMouseDown = (e: MouseEvent) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement | null;
    if (target?.closest("button, input, textarea, select, a")) return;
    win.startDragging().catch((err) => {
      console.warn("[titlebar] startDragging failed:", err);
    });
  };
  const onDragDblClick = (e: MouseEvent) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest("button, input, textarea, select, a")) return;
    onMaximize();
  };

  return (
    <div
      data-tauri-drag-region
      data-testid="titlebar"
      onMouseDown={onDragMouseDown}
      onDblClick={onDragDblClick}
      style={{
        display: "flex",
        "align-items": "center",
        height: "32px",
        background: "var(--color-surface)",
        "border-bottom": "1px solid var(--color-border)",
        "user-select": "none",
        "flex-shrink": 0,
        // macOS:traffic lights 在左上 ~78px 区域,让出位置避免被覆盖
        "padding-left": IS_MAC ? "78px" : "0",
      }}
    >
      <div
        style={{
          display: "flex",
          "align-items": "center",
          gap: "8px",
          padding: "0 12px",
          color: "var(--color-text)",
          "font-size": "12px",
          "font-weight": 600,
        }}
      >
        {props.left}
      </div>
      <div
        style={{
          flex: 1,
          "text-align": "center",
          color: "var(--color-text-2)",
          "font-size": "12px",
        }}
      >
        {props.center ?? props.title}
      </div>
      {props.right ? (
        <div
          data-testid="titlebar-right"
          style={{
            display: "flex",
            "align-items": "center",
            gap: "4px",
            padding: "0 8px",
          }}
        >
          {props.right}
        </div>
      ) : null}
      {/* macOS 走原生 traffic lights,不渲我们的 Win 按钮 */}
      {IS_MAC ? null : (
        <div style={{ display: "flex", "align-items": "stretch" }}>
          <WinBtn onClick={onMinimize} title={t("titlebar.minimize")}>
            <Minus size={12} />
          </WinBtn>
          <WinBtn onClick={onMaximize} title={maximized() ? t("titlebar.restore") : t("titlebar.maximize")}>
            <Square size={10} />
          </WinBtn>
          <WinBtn onClick={onClose} danger title={t("titlebar.close")}>
            <X size={12} />
          </WinBtn>
        </div>
      )}
    </div>
  );
};

const WinBtn: Component<{
  onClick: () => void;
  title: string;
  danger?: boolean;
  children: JSX.Element;
}> = (p) => {
  const [hover, setHover] = createSignal(false);
  return (
    <button
      onClick={p.onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      title={p.title}
      style={{
        background: hover()
          ? p.danger
            ? "#e81123"
            : "var(--color-accent-subtle)"
          : "transparent",
        color: hover() && p.danger ? "#fff" : "var(--color-text-2)",
        border: "none",
        width: "44px",
        height: "32px",
        display: "flex",
        "align-items": "center",
        "justify-content": "center",
        cursor: "pointer",
        outline: "none",
      }}
    >
      {p.children}
    </button>
  );
};
