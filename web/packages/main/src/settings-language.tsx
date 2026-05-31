// 语言选择 — 14 语网格,当前高亮,点击即切换并持久化(setLang)。
// 切换后全局 currentLang 变更,t() 响应式触发整个 UI 文案重渲染。
import { For, Show, type Component } from "solid-js";
import { Check } from "lucide-solid";
import { t, LANGS, LANG_NAMES, currentLang, setLang } from "@vibeterm/ui-core";

export const LanguageTab: Component = () => {
  return (
    <div>
      <h3 style={{ margin: "0 0 6px 0", "font-size": "14px" }}>{t("settings.tab.language")}</h3>
      <p style={{ margin: "0 0 14px 0", "font-size": "12px", color: "var(--color-text-2)" }}>
        {t("settings.language.hint")}
      </p>
      <div
        style={{
          display: "grid",
          "grid-template-columns": "repeat(auto-fill, minmax(180px, 1fr))",
          gap: "8px",
        }}
      >
        <For each={LANGS}>
          {(l) => {
            const active = () => currentLang() === l;
            return (
              <div
                data-testid={`lang-card-${l}`}
                data-lang={l}
                data-active={active() ? "true" : "false"}
                onClick={() => setLang(l)}
                style={{
                  display: "flex",
                  "align-items": "center",
                  "justify-content": "space-between",
                  gap: "8px",
                  padding: "10px 12px",
                  cursor: "pointer",
                  "border-radius": "6px",
                  "font-size": "13px",
                  color: "var(--color-text)",
                  border: active()
                    ? "2px solid var(--color-accent)"
                    : "1px solid var(--color-border)",
                  background: active() ? "var(--color-accent-subtle)" : "var(--color-bg)",
                }}
              >
                <span>{LANG_NAMES[l]}</span>
                <Show when={active()}>
                  <Check size={14} style={{ color: "var(--color-accent)" }} />
                </Show>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
};
