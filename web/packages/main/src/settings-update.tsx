// 更新页 — 两个独立的手动检查:软件本身(仅版本对比 + release 链接)、模型价格(下载并应用)。
//
// 🔴 零侵入:两块都仅在用户点按钮时联网(后端 ureq GET 固定端点),无后台轮询、无上传。
import { type Component, Show, createSignal, onMount } from "solid-js";
import { Package, DollarSign, RefreshCw, Download, RotateCcw, Check } from "lucide-solid";
import { ipc, t } from "@vibeterm/ui-core";
import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
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
  // GitHub 限流(403/429)单独给"稍后重试"文案 —— 这种失败网络是通的,别误导用户查网络
  const [appRateLimited, setAppRateLimited] = createSignal(false);

  const checkApp = async () => {
    setAppState("checking");
    setAppErr("");
    setAppRateLimited(false);
    try {
      setAppInfo(await ipc.checkAppUpdate());
      setAppState("done");
    } catch (e) {
      const msg = ipc.formatIpcError(e);
      // 后端约定:限流的 trace_id 带 ":rate_limited:" 标记(updates.rs)
      setAppRateLimited(msg.includes(":rate_limited:"));
      setAppErr(msg);
      setAppState("error");
    }
  };

  // ---- 应用内下载并安装(Sparkle 等价物)----
  // 🔴 零侵入:check()/downloadAndInstall() 仅在此函数(用户点按钮)里调用,无启动期/后台自动触发。
  // updater 用 minisign 校验签名后原地更新;失败则回退到「打开下载页」。
  const [installState, setInstallState] =
    createSignal<"idle" | "downloading" | "installing" | "done" | "error">("idle");
  const [downloadPct, setDownloadPct] = createSignal(0);

  const downloadAndInstall = async () => {
    setInstallState("downloading");
    setDownloadPct(0);
    try {
      const update = await check();
      if (!update) {
        // latest.json 缺失 / 未签名 / 版本不更新 → 当作"应用内安装不可用",回退打开下载页
        setInstallState("error");
        return;
      }
      let total = 0;
      let downloaded = 0;
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? 0;
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            if (total > 0) setDownloadPct(Math.min(100, Math.round((downloaded / total) * 100)));
            break;
          case "Finished":
            setInstallState("installing");
            break;
        }
      });
      setInstallState("done");
      await relaunch();
    } catch (e) {
      console.error("[update] downloadAndInstall", e);
      setInstallState("error");
    }
  };

  // ---- 启动自动检查开关 + 安装前"运行中"护栏 ----
  const [autoCheck, setAutoCheck] = createSignal(true);
  const [confirmInstall, setConfirmInstall] = createSignal(false);
  const [runningCount, setRunningCount] = createSignal(0);

  const toggleAutoCheck = async (v: boolean) => {
    setAutoCheck(v);
    try {
      await ipc.setAutoCheckUpdates(v);
    } catch (e) {
      console.error("[update] setAutoCheckUpdates", e);
    }
  };

  // 点"下载并安装":若有终端正在运行(running / waiting_input),先确认再装,防打断 agent。
  const requestInstall = async () => {
    if (!confirmInstall()) {
      try {
        const tasks = await ipc.listTasks();
        const n = tasks.filter(
          (t) => t.status === "running" || t.status === "waiting_input",
        ).length;
        if (n > 0) {
          setRunningCount(n);
          setConfirmInstall(true);
          return;
        }
      } catch (e) {
        console.error("[update] listTasks", e);
      }
    }
    setConfirmInstall(false);
    await downloadAndInstall();
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
    // 读自动检查开关;若开,进页面主动查一次(展示当前状态)。
    try {
      const cfg = await ipc.getConfig();
      setAutoCheck(cfg.auto_check_updates);
      if (cfg.auto_check_updates) void checkApp();
    } catch (e) {
      console.error("[update] getConfig", e);
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
              <div style={{ display: "flex", gap: "8px", "align-items": "center", "flex-wrap": "wrap", "margin-top": "10px" }}>
                <button
                  data-testid="update-download-install"
                  style={btn(true)}
                  onClick={requestInstall}
                  disabled={installState() === "downloading" || installState() === "installing" || installState() === "done"}
                >
                  <Download size={13} />{" "}
                  {installState() === "downloading"
                    ? t("update.app.downloading", { pct: String(downloadPct()) })
                    : installState() === "installing"
                      ? t("update.app.installing")
                      : installState() === "done"
                        ? t("update.app.install_done")
                        : t("update.app.download_install")}
                </button>
                <button
                  data-testid="update-open-download"
                  style={btn(false)}
                  onClick={() => appInfo()!.release_url && ipc.openExternal(appInfo()!.release_url!).catch(console.error)}
                >
                  {t("update.app.open_download")}
                </button>
              </div>
              {/* 安装前护栏:有终端运行中 → 二次确认,防打断 agent(任务列表不丢,仅中断进程) */}
              <Show when={confirmInstall()}>
                <div style={{ "margin-top": "10px", padding: "10px 12px", background: "var(--color-surface)", border: "1px solid var(--color-status-waiting, #e5a23d)", "border-radius": "8px" }}>
                  <div style={{ "font-size": "12px", color: "var(--color-text)", "line-height": 1.5 }}>
                    {t("update.app.running_warn", { count: String(runningCount()) })}
                  </div>
                  <div style={{ display: "flex", gap: "8px", "margin-top": "8px" }}>
                    <button data-testid="update-install-anyway" style={btn(true)} onClick={requestInstall}>
                      {t("update.app.install_anyway")}
                    </button>
                    <button style={btn(false)} onClick={() => setConfirmInstall(false)}>
                      {t("dialog.cancel")}
                    </button>
                  </div>
                </div>
              </Show>
              <Show when={installState() === "error"}>
                <div style={{ "margin-top": "8px", "font-size": "12px", color: "var(--color-status-waiting, #e5a23d)" }}>
                  {t("update.app.install_error")}
                </div>
              </Show>
            </div>
          </Show>
        </Show>

        <Show when={appState() === "error"}>
          <div style={{ "margin-top": "12px", "font-size": "12px", color: "var(--color-status-waiting, #e5a23d)" }}>
            {appRateLimited() ? t("update.app.rate_limited") : t("update.app.error")}
            <Show when={appErr()}>
              <span style={{ color: "var(--color-text-2)", "margin-left": "6px", "font-family": "monospace", "font-size": "11px" }}>{appErr()}</span>
            </Show>
          </div>
        </Show>

        <p style={note()}>{t("update.app.note")}</p>

        {/* 启动自动检查开关(默认开;关闭后开箱完全不主动联网) */}
        <label style={{ display: "flex", "align-items": "flex-start", gap: "8px", "margin-top": "12px", cursor: "pointer" }}>
          <input
            data-testid="update-auto-check"
            type="checkbox"
            checked={autoCheck()}
            onChange={(e) => toggleAutoCheck(e.currentTarget.checked)}
            style={{ "margin-top": "2px" }}
          />
          <span style={{ "font-size": "12px", color: "var(--color-text)" }}>
            {t("update.app.auto_check")}
            <span style={{ display: "block", "font-size": "11px", color: "var(--color-text-2)", "margin-top": "2px" }}>
              {t("update.app.auto_check_hint")}
            </span>
          </span>
        </label>
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
