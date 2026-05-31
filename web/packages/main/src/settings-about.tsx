// 关于页 — 介绍 + 设计理念 + 开源致谢。纯静态,不联网。
//
// 致谢力求全面:直接依赖(Cargo.toml / package.json)+ 灵感借鉴(代码注释标注的参考实现)
// + 配色主题 + 字体 / 音效。完整许可见 THIRD-PARTY-NOTICES.md。
import { type Component, For, Show, createSignal, onMount } from "solid-js";
import { Github, Heart, Shield, Scale, Lightbulb, Sparkles } from "lucide-solid";
import { ipc, t } from "@vibeterm/ui-core";
import { getVersion } from "@tauri-apps/api/app";

const REPO_URL = "https://github.com/fjlmcm/VibeTerm";

/** 核心特性(功能层)—— 分组 i18n key。 */
const FEATURES: { group: string; items: string[] }[] = [
  {
    group: "features.group.agent",
    items: ["features.agent.sniff", "features.agent.urgency", "features.agent.monitor", "features.agent.stats"],
  },
  {
    group: "features.group.terminal",
    items: ["features.terminal.split", "features.terminal.canvas", "features.terminal.floating", "features.terminal.render"],
  },
  {
    group: "features.group.productivity",
    items: [
      "features.productivity.palette",
      "features.productivity.prompts",
      "features.productivity.statusbar",
      "features.productivity.notify",
      "features.productivity.theme",
    ],
  },
];

/** 设计理念(i18n key)—— 理念层(为什么),与核心特性(是什么)分工。 */
const PRINCIPLES = [
  "about.principle.zero_intrusion",
  "about.principle.multi_agent",
  "about.principle.terminal",
  "about.principle.cjk",
  "about.principle.privacy",
  "about.principle.oss",
];

/** 灵感与借鉴:代码里标注参考过的开源项目 + 借鉴点(note 为 i18n key)。
 *  url 只填确证的官方仓库;无法确证的留空(只列名 + 借鉴点,不编造链接)。 */
const INSPIRATION: { name: string; by?: string; url?: string; note: string }[] = [
  { name: "ccusage", by: "ryoppippi", url: "https://github.com/ryoppippi/ccusage", note: "about.inspire.ccusage" },
  { name: "WezTerm", by: "wez", url: "https://github.com/wez/wezterm", note: "about.inspire.wezterm" },
  { name: "Tabby", by: "Eugeny", url: "https://github.com/Eugeny/tabby", note: "about.inspire.tabby" },
  { name: "LiteLLM", by: "BerriAI", url: "https://github.com/BerriAI/litellm", note: "about.inspire.litellm" },
  { name: "Prowl", note: "about.inspire.prowl" },
  { name: "CodexBar", note: "about.inspire.codexbar" },
  { name: "ccstatusline", note: "about.inspire.ccstatusline" },
  { name: "panzoom", note: "about.inspire.panzoom" },
];

/** 技术栈致谢分组:组标题 i18n key + 依赖名(专有名词不翻译)。 */
const CREDITS: { group: string; items: string[] }[] = [
  { group: "about.credits.framework", items: ["Tauri 2", "SolidJS", "xterm.js", "WebGL / fit / search / web-links / unicode-graphemes"] },
  { group: "about.credits.rust", items: ["portable-pty", "tokio", "serde", "notify", "chrono", "tracing", "thiserror", "image", "blake3", "base64", "ureq", "which", "tempfile", "cocoa"] },
  { group: "about.credits.frontend", items: ["lucide-solid", "solid-dnd", "html-to-image", "@tauri-apps/api", "Vite"] },
  { group: "about.credits.themes", items: ["Gruvbox", "Nord", "Tokyo Night", "Catppuccin", "Solarized"] },
];

const card = (): Record<string, string> => ({
  background: "var(--color-bg)",
  border: "1px solid var(--color-border)",
  "border-radius": "10px",
  padding: "16px 18px",
});

const sectionTitle = (): Record<string, string> => ({
  display: "flex",
  "align-items": "center",
  gap: "6px",
  "font-size": "12px",
  "font-weight": "600",
  color: "var(--color-text-2)",
  "text-transform": "uppercase",
  "letter-spacing": "0.6px",
  margin: "0 0 12px 0",
});

const chip = (): Record<string, string> => ({
  display: "inline-block",
  padding: "3px 9px",
  "font-size": "12px",
  background: "var(--color-surface)",
  border: "1px solid var(--color-border)",
  "border-radius": "999px",
  color: "var(--color-text)",
});

const linkBtn = (): Record<string, string> => ({
  display: "inline-flex",
  "align-items": "center",
  gap: "6px",
  padding: "7px 14px",
  "font-size": "13px",
  "font-weight": "500",
  background: "var(--color-surface)",
  border: "1px solid var(--color-border)",
  "border-radius": "7px",
  color: "var(--color-text)",
  cursor: "pointer",
});

export const AboutTab: Component = () => {
  const [version, setVersion] = createSignal("");
  onMount(async () => {
    try {
      setVersion(await getVersion());
    } catch {
      /* dev 模式可能取不到 — 忽略 */
    }
  });
  const open = (url: string) => ipc.openExternal(url).catch((e) => console.error("[about] open", e));

  return (
    <div style={{ display: "flex", "flex-direction": "column", gap: "18px", "max-width": "800px" }}>
      {/* 品牌 hero */}
      <div
        style={{
          position: "relative",
          overflow: "hidden",
          "border-radius": "14px",
          padding: "28px 26px",
          background: "linear-gradient(135deg, var(--color-accent-subtle), var(--color-bg) 72%)",
          border: "1px solid var(--color-border)",
        }}
      >
        <div style={{ display: "flex", "align-items": "baseline", gap: "12px" }}>
          <span style={{ "font-size": "30px", "font-weight": "700", color: "var(--color-text)", "letter-spacing": "-0.5px" }}>
            VibeTerm
          </span>
          <Show when={version()}>
            <span style={{ "font-size": "13px", "font-family": "monospace", color: "var(--color-accent)", "font-weight": "600" }}>
              v{version()}
            </span>
          </Show>
        </div>
        <div style={{ "margin-top": "10px", "font-size": "14px", color: "var(--color-text-2)", "line-height": "1.6", "max-width": "640px", "white-space": "pre-line" }}>
          {t("about.tagline")}
        </div>
      </div>

      {/* 设计理念 */}
      <section style={card()}>
        <h4 style={sectionTitle()}>
          <Shield size={12} /> {t("about.principles.title")}
        </h4>
        <ul style={{ margin: 0, padding: 0, "list-style": "none", display: "flex", "flex-direction": "column", gap: "9px" }}>
          <For each={PRINCIPLES}>
            {(key) => (
              <li style={{ "font-size": "13px", color: "var(--color-text)", "line-height": "1.5", "padding-left": "16px", position: "relative" }}>
                <span style={{ position: "absolute", left: 0, color: "var(--color-accent)", "font-weight": "700" }}>·</span>
                {t(key)}
              </li>
            )}
          </For>
        </ul>
      </section>

      {/* 核心特性 */}
      <section style={card()}>
        <h4 style={sectionTitle()}>
          <Sparkles size={12} /> {t("features.title")}
        </h4>
        <div style={{ display: "flex", "flex-direction": "column", gap: "16px" }}>
          <For each={FEATURES}>
            {(grp) => (
              <div>
                <div style={{ "font-size": "11px", "font-weight": "600", color: "var(--color-accent)", "margin-bottom": "8px", "letter-spacing": "0.3px" }}>
                  {t(grp.group)}
                </div>
                <ul style={{ margin: 0, padding: 0, "list-style": "none", display: "flex", "flex-direction": "column", gap: "7px" }}>
                  <For each={grp.items}>
                    {(key) => (
                      <li style={{ "font-size": "13px", color: "var(--color-text)", "line-height": "1.5", "padding-left": "16px", position: "relative" }}>
                        <span style={{ position: "absolute", left: 0, color: "var(--color-text-2)" }}>·</span>
                        {t(key)}
                      </li>
                    )}
                  </For>
                </ul>
              </div>
            )}
          </For>
        </div>
      </section>

      {/* 特别致敬 ccusage */}
      <section style={{ ...card(), "border-color": "var(--color-accent)" }}>
        <h4 style={sectionTitle()}>
          <Heart size={12} style={{ color: "var(--color-accent)" }} /> {t("about.special_thanks.title")}
        </h4>
        <p style={{ margin: "0", "font-size": "13px", color: "var(--color-text)", "line-height": "1.55" }}>
          {t("about.special_thanks.ccusage")}
        </p>
      </section>

      {/* 灵感与借鉴 */}
      <section style={card()}>
        <h4 style={sectionTitle()}>
          <Lightbulb size={12} /> {t("about.inspiration.title")}
        </h4>
        <p style={{ margin: "0 0 12px 0", "font-size": "12px", color: "var(--color-text-2)", "line-height": "1.5" }}>
          {t("about.inspiration.desc")}
        </p>
        <div style={{ display: "flex", "flex-direction": "column", gap: "8px" }}>
          <For each={INSPIRATION}>
            {(it) => (
              <div style={{ "font-size": "13px", "line-height": "1.45" }}>
                <span
                  style={{
                    "font-weight": "600",
                    color: it.url ? "var(--color-accent)" : "var(--color-text)",
                    cursor: it.url ? "pointer" : "default",
                  }}
                  onClick={() => it.url && open(it.url)}
                >
                  {it.name}
                </span>
                <Show when={it.by}>
                  <span style={{ color: "var(--color-text-2)", "font-size": "11px" }}> · {it.by}</span>
                </Show>
                <span style={{ color: "var(--color-text-2)" }}> — {t(it.note)}</span>
              </div>
            )}
          </For>
        </div>
      </section>

      {/* 技术栈致谢 */}
      <section style={card()}>
        <h4 style={sectionTitle()}>{t("about.credits.title")}</h4>
        <div style={{ display: "flex", "flex-direction": "column", gap: "14px" }}>
          <For each={CREDITS}>
            {(grp) => (
              <div>
                <div style={{ "font-size": "11px", color: "var(--color-text-2)", "margin-bottom": "7px" }}>{t(grp.group)}</div>
                <div style={{ display: "flex", "flex-wrap": "wrap", gap: "6px" }}>
                  <For each={grp.items}>{(name) => <span style={chip()}>{name}</span>}</For>
                </div>
              </div>
            )}
          </For>
        </div>
        <p style={{ margin: "14px 0 0 0", "font-size": "11px", color: "var(--color-text-2)", "line-height": "1.5" }}>
          {t("about.credits.foot")}
        </p>
      </section>

      {/* 链接 + 版权 */}
      <div style={{ display: "flex", "align-items": "center", "flex-wrap": "wrap", gap: "10px" }}>
        <span style={linkBtn()} onClick={() => open(REPO_URL)}>
          <Github size={14} /> {t("about.link.repo")}
        </span>
        <span style={{ display: "inline-flex", "align-items": "center", gap: "6px", "font-size": "12px", color: "var(--color-text-2)" }}>
          <Scale size={13} /> {t("about.license")}
        </span>
        <div style={{ flex: 1 }} />
        <span style={{ "font-size": "11px", color: "var(--color-text-2)" }}>{t("about.copyright")}</span>
      </div>
    </div>
  );
};
