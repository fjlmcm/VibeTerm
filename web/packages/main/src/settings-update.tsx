// 更新页 — 两个独立的手动检查:软件本身(仅版本对比 + release 链接)、模型价格(下载并应用)。
//
// 🔴 零侵入:两块都仅在用户点按钮时联网(后端 ureq GET 固定端点),无后台轮询、无上传。
import { type Component, Show, createSignal, onMount } from "solid-js";
import { Package, DollarSign, RefreshCw, Download, RotateCcw, Check } from "lucide-solid";
import { ipc, t } from "@vibeterm/ui-core";
import { getVersion } from "@tauri-apps/api/app";
import type { AppUpdateInfo, PricingStatus } from "@vibeterm/ipc-types";

const card = (): Record<string, string> => ({
  background: "var(--color-bg)",
  border: "1px solid var(--color-border)",
  "border-radius": "10px",
  padding: "18px 18px",
});

const sectionTitle = (): Record<string, string> => ({
  display: "flex",
  "align-items": "center",
  gap: "7px",
  "font-size": "14px",
  "font-weight": "600",
  color: "var(--color-text)",
  margin: "0 0 4px 0",
});

const btn = (primary?: boolean): Record<string, string> => ({
  display: "inline-flex",
  "align-items": "center",
  gap: "6px",
  padding: "7px 14px",
  "font-size": "13px",
  "font-weight": "500",
  background: primary ? "var(--color-accent)" : "var(--color-surface)",
  color: primary ? "var(--color-bg)" : "var(--color-text)",
  border: primary ? "none" : "1px solid var(--color-border)",
  "border-radius": "7px",
  cursor: "pointer",
});

const note = (): Record<string, string> => ({
  "font-size": "11px",
  color: "var(--color-text-2)",
  "line-height": "1.5",
  margin: "10px 0 0 0",
});

export const UpdateTab: Component = () => {
  const [version, setVersion] = createSignal("");

  // ---- 软件更新 ----
  const [appState, setAppState] = createSignal<"idle" | "checking" | "done" | "error">("idle");
  const [appInfo, setAppInfo] = createSignal<AppUpdateInfo | null>(null);
  const [appErr, setAppErr] = createSignal("");

  const checkApp = async () => {
    setAppState("checking");
    setAppErr("");
    try {
      setAppInfo(await ipc.checkAppUpdate());
      setAppState("done");
    } catch (e) {
      setAppErr(String(e));
      setAppState("error");
    }
  };

  // ---- 模型价格 ----
  const [pricing, setPricing] = createSignal<PricingStatus | null>(null);
  const [priceState, setPriceState] = createSignal<"idle" | "updating">("idle");
  const [priceMsg, setPriceMsg] = createSignal("");
  const [priceErr, setPriceErr] = createSignal(false);

  const loadPricing = async () => {
    try {
      setPricing(await ipc.getPricingStatus());
    } catch (e) {
      console.error("[update] getPricingStatus", e);
    }
  };
  const updatePricing = async () => {
    setPriceState("updating");
    setPriceMsg("");
    setPriceErr(false);
    try {
      const s = await ipc.updateModelPricing();
      setPricing(s);
      setPriceMsg(t("update.price.updated", { date: s.updated_at ?? "" }));
    } catch (e) {
      setPriceErr(true);
      setPriceMsg(t("update.price.failed"));
      console.error("[update] updateModelPricing", e);
    } finally {
      setPriceState("idle");
    }
  };
  const resetPricing = async () => {
    try {
      setPricing(await ipc.resetModelPricing());
      setPriceMsg("");
      setPriceErr(false);
    } catch (e) {
      console.error("[update] resetModelPricing", e);
    }
  };

  onMount(async () => {
    try {
      setVersion(await getVersion());
    } catch {
      /* ignore */
    }
    await loadPricing();
  });

  return (
    <div style={{ display: "flex", "flex-direction": "column", gap: "18px", "max-width": "760px" }}>
      {/* ===== 软件更新 ===== */}
      <section style={card()}>
        <h3 style={sectionTitle()}>
          <Package size={15} /> {t("update.app.title")}
        </h3>
        <div style={{ "font-size": "12px", color: "var(--color-text-2)", "margin-bottom": "12px" }}>
          {t("update.app.current")}: <span style={{ "font-family": "monospace", color: "var(--color-text)" }}>v{version()}</span>
        </div>

        <button data-testid="update-check-app" style={btn(true)} onClick={checkApp} disabled={appState() === "checking"}>
          <RefreshCw size={13} /> {appState() === "checking" ? t("update.app.checking") : t("update.app.check")}
        </button>

        <Show when={appState() === "done" && appInfo()}>
          <Show
            when={appInfo()!.has_update}
            fallback={
              <div style={{ "margin-top": "12px", "font-size": "13px", color: "var(--color-status-running, var(--color-accent))", display: "flex", "align-items": "center", gap: "6px" }}>
                <Check size={14} /> {t("update.app.up_to_date")}
              </div>
            }
          >
            <div style={{ "margin-top": "14px", padding: "12px 14px", background: "var(--color-accent-subtle)", "border-radius": "8px", border: "1px solid var(--color-accent)" }}>
              <div style={{ "font-size": "13px", "font-weight": "600", color: "var(--color-text)" }}>
                {t("update.app.new_version", { version: appInfo()!.latest ?? "" })}
              </div>
              <Show when={appInfo()!.notes}>
                <pre style={{ margin: "8px 0 0 0", "font-size": "11px", color: "var(--color-text-2)", "white-space": "pre-wrap", "max-height": "160px", overflow: "auto", "font-family": "inherit" }}>
                  {appInfo()!.notes}
                </pre>
              </Show>
              <button
                data-testid="update-open-download"
                style={{ ...btn(true), "margin-top": "10px" }}
                onClick={() => appInfo()!.release_url && ipc.openExternal(appInfo()!.release_url!).catch(console.error)}
              >
                <Download size={13} /> {t("update.app.open_download")}
              </button>
            </div>
          </Show>
        </Show>

        <Show when={appState() === "error"}>
          <div style={{ "margin-top": "12px", "font-size": "12px", color: "var(--color-status-waiting, #e5a23d)" }}>
            {t("update.app.error")}
            <Show when={appErr()}>
              <span style={{ color: "var(--color-text-2)", "margin-left": "6px", "font-family": "monospace", "font-size": "11px" }}>{appErr()}</span>
            </Show>
          </div>
        </Show>

        <p style={note()}>{t("update.app.note")}</p>
      </section>

      {/* ===== 模型价格 ===== */}
      <section style={card()}>
        <h3 style={sectionTitle()}>
          <DollarSign size={15} /> {t("update.price.title")}
        </h3>

        {/* 用途说明(显著) */}
        <div
          style={{
            "font-size": "12px",
            color: "var(--color-text)",
            "line-height": 1.6,
            background: "var(--color-surface)",
            padding: "10px 12px",
            "border-radius": "8px",
            border: "1px solid var(--color-border)",
            "margin-bottom": "14px",
          }}
        >
          {t("update.price.purpose")}
        </div>

        <div style={{ "font-size": "12px", color: "var(--color-text-2)", "margin-bottom": "12px" }}>
          {t("update.price.current")}:{" "}
          <Show
            when={pricing()?.source === "override"}
            fallback={<span style={{ color: "var(--color-text)" }}>{t("update.price.builtin")}</span>}
          >
            <span style={{ color: "var(--color-text)" }}>
              {t("update.price.updated_label", { date: pricing()?.updated_at ?? "" })}
            </span>
          </Show>
        </div>

        <div style={{ display: "flex", gap: "8px", "align-items": "center", "flex-wrap": "wrap" }}>
          <button data-testid="update-pricing" style={btn(true)} onClick={updatePricing} disabled={priceState() === "updating"}>
            <RefreshCw size={13} /> {priceState() === "updating" ? t("update.price.updating") : t("update.price.check")}
          </button>
          <Show when={pricing()?.source === "override"}>
            <button data-testid="reset-pricing" style={btn(false)} onClick={resetPricing}>
              <RotateCcw size={13} /> {t("update.price.reset")}
            </button>
          </Show>
          <Show when={priceMsg()}>
            <span style={{ "font-size": "12px", color: priceErr() ? "var(--color-status-waiting, #e5a23d)" : "var(--color-text-2)" }}>
              {priceMsg()}
            </span>
          </Show>
        </div>

        <p style={note()}>{t("update.price.note")}</p>
      </section>
    </div>
  );
};
