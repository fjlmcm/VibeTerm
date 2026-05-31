// 模型使用统计面板(modal)
//
// 全量聚合 ~/.claude/projects + ~/.codex/sessions 的历史用量, 离线估算成本.
// 两用: 自己看 + 截图发社媒. 故分成"截图卡片区"(品牌 hero + KPI + 图表 + 水印)
// 与"控件工具栏"(范围 / 隐藏项目 / 保存图片) —— 控件不入截图.
//
// 项目路径默认**模糊**(眼睛闭合), 截图不泄露本地路径; 点睁眼临时显示.
// 全部走 design tokens, 不硬编码配色(SERIES 的 var() fallback 仅为缺主题变量时兜底).

import { For, Show, createMemo, createResource, createSignal, type Component, type ParentComponent, type JSX } from "solid-js";
import { ArrowLeft, Camera, Eye, EyeOff, Coins, Hash, MessageSquare, Cpu, Check } from "lucide-solid";
import { toPng } from "html-to-image";
import { save } from "@tauri-apps/plugin-dialog";
import { ipc, t, Titlebar } from "@vibeterm/ui-core";
import type { UsageStats, UsageDailyStat, UsageModelStat, UsageProjectStat } from "@vibeterm/ipc-types";

export interface StatsPanelProps {
  onClose: () => void;
}

const RANGES = [7, 30, 90] as const;

/** 系列配色 — 走主题 token(带兜底), 按出现顺序稳定取色. */
const SERIES = [
  "var(--color-accent)",
  "var(--color-status-running, #689d6a)",
  "var(--color-status-waiting, #d79921)",
  "var(--color-status-stalled, #d97757)",
  "var(--color-accent-subtle)",
  "var(--color-text-2)",
];
const CLAUDE_COLOR = "var(--color-accent)";
const CODEX_COLOR = "var(--color-status-running, #689d6a)";

// 骨架屏扫光动画 — 用主题 token(bg→border→bg)做渐变扫过, 不硬编码配色.
const SHIMMER_CSS = "@keyframes vt-shimmer{0%{background-position:200% 0}100%{background-position:-200% 0}}";
const SHIMMER =
  "linear-gradient(90deg, var(--color-bg) 25%, var(--color-border) 50%, var(--color-bg) 75%)";
// 首扫骨架的趋势柱高度(固定图案, 非随机, 渲染稳定).
const SK_BARS = [42, 65, 30, 78, 50, 88, 35, 60, 72, 45, 92, 55, 38, 68, 48, 80, 58, 33, 70, 52, 85, 40, 62, 75, 47, 90, 36, 66];

function fmtTokens(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0";
  if (n >= 1e9) return (n / 1e9).toFixed(2) + "B";
  if (n >= 1e6) return (n / 1e6).toFixed(2) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "k";
  return String(Math.round(n));
}

function fmtCost(n: number | null): string {
  return n == null || !Number.isFinite(n) ? "—" : "$" + n.toFixed(2);
}

function fmtPct(frac: number): string {
  if (!Number.isFinite(frac)) return "0%";
  const p = frac * 100;
  return (p < 10 ? p.toFixed(1) : Math.round(p).toString()) + "%";
}

/** YYYY-MM-DD → MM-DD */
function shortDate(d: string): string {
  return d.length >= 10 ? d.slice(5) : d;
}

function localDate(ms: number): string {
  try {
    return new Date(ms).toLocaleDateString();
  } catch {
    return "";
  }
}

/** Date → 本地 YYYY-MM-DD (与后端 chrono::Local 分天对齐). */
function ymdLocal(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

/** data-testid 安全化 — 路径/模型名含空格/斜杠/点会破坏 CSS 选择器. */
function tid(s: string): string {
  return s.replace(/[^a-zA-Z0-9_-]/g, "-");
}

export const StatsPanel: Component<StatsPanelProps> = (props) => {
  const [days, setDays] = createSignal<number>(30);
  const [stats] = createResource(days, (d) => ipc.getUsageStats(d));
  const [reveal, setReveal] = createSignal(false); // 项目路径默认隐藏(模糊)
  const [saveState, setSaveState] = createSignal<"idle" | "saving" | "saved" | "error">("idle");

  let cardRef: HTMLDivElement | undefined;

  const hasData = (): boolean => {
    const s = stats();
    return !!s && s.totals.message_count + s.totals.codex_tokens > 0;
  };

  const saveImage = async () => {
    const node = cardRef;
    if (!node || saveState() === "saving") return;
    setSaveState("saving");
    try {
      const surface =
        getComputedStyle(document.documentElement).getPropertyValue("--color-surface").trim() ||
        "#282828";
      const dataUrl = await toPng(node, { pixelRatio: 2, backgroundColor: surface, cacheBust: true });
      const base64 = dataUrl.split(",")[1] ?? "";
      const path = await save({
        defaultPath: `vibeterm-usage-${days()}d.png`,
        filters: [{ name: "PNG", extensions: ["png"] }],
      });
      if (!path) {
        setSaveState("idle"); // 用户取消
        return;
      }
      await ipc.savePngFile(path, base64);
      setSaveState("saved");
      setTimeout(() => setSaveState("idle"), 2500);
    } catch (e) {
      console.error("[stats] save image failed", e);
      setSaveState("error");
      setTimeout(() => setSaveState("idle"), 2500);
    }
  };

  return (
    <div
      data-testid="stats-panel"
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--color-surface)",
        "z-index": 2000,
        display: "grid",
        "grid-template-rows": "auto 1fr",
        "min-height": 0,
      }}
    >
      <Titlebar
        center={
          <span style={{ "font-weight": 600, color: "var(--color-text)", "font-size": "12px" }}>
            {t("stats.title")}
          </span>
        }
        right={
          <button
            data-testid="stats-back"
            onClick={props.onClose}
            title={t("settings.back")}
            style={backBtnStyle()}
          >
            {t("settings.back")} <ArrowLeft size={12} style={{ transform: "rotate(180deg)" }} />
          </button>
        }
      />

      <div style={{ "overflow-y": "auto", "min-height": 0, padding: "20px" }}>
        <div style={{ "max-width": "920px", margin: "0 auto" }}>
          {/* 控件工具栏 — 不入截图 */}
          <div
            style={{
              display: "flex",
              "align-items": "center",
              "justify-content": "space-between",
              gap: "10px",
              "flex-wrap": "wrap",
              "margin-bottom": "14px",
            }}
          >
            <div data-testid="stats-range" style={{ display: "flex", gap: "4px", "align-items": "center" }}>
              <span style={{ "font-size": "11px", color: "var(--color-text-2)", "margin-right": "2px" }}>
                {t("stats.range.label")}
              </span>
              <For each={RANGES}>
                {(d) => (
                  <button
                    data-testid={`stats-range-${d}`}
                    onClick={() => setDays(d)}
                    style={pillStyle(days() === d)}
                  >
                    {t("stats.range.days", { n: d })}
                  </button>
                )}
              </For>
            </div>
            <div style={{ display: "flex", gap: "6px", "align-items": "center" }}>
              <button
                data-testid="stats-reveal-toggle"
                onClick={() => setReveal((v) => !v)}
                title={reveal() ? t("stats.hide") : t("stats.reveal")}
                style={toolBtnStyle()}
              >
                {reveal() ? <Eye size={13} /> : <EyeOff size={13} />}
                <span>{reveal() ? t("stats.hide") : t("stats.reveal")}</span>
              </button>
              <button
                data-testid="stats-save-image"
                onClick={saveImage}
                disabled={saveState() === "saving" || !hasData()}
                title={t("stats.save_image")}
                style={toolBtnStyle(true)}
              >
                {saveState() === "saved" ? <Check size={13} /> : <Camera size={13} />}
                <span>
                  {saveState() === "saving"
                    ? t("stats.saving")
                    : saveState() === "saved"
                      ? t("stats.saved")
                      : saveState() === "error"
                        ? t("stats.save_failed")
                        : t("stats.save_image")}
                </span>
              </button>
            </div>
          </div>

          <Show when={!stats.loading} fallback={<StatsSkeleton />}>
            <Show
              when={hasData()}
              fallback={<div data-testid="stats-empty" style={emptyStyle()}>{t("stats.empty", { n: days() })}</div>}
            >
              {/* 截图卡片区 — cardRef 即导出的图像范围 */}
              <div
                ref={(el) => (cardRef = el)}
                style={{
                  background: "var(--color-surface)",
                  border: "1px solid var(--color-border)",
                  "border-radius": "14px",
                  padding: "26px 28px",
                }}
              >
                {/* 品牌 hero */}
                <div style={{ "border-bottom": "1px solid var(--color-border)", "padding-bottom": "16px", "margin-bottom": "18px" }}>
                  <div style={{ "font-size": "11px", "font-weight": 700, "letter-spacing": "0.14em", "text-transform": "uppercase", color: "var(--color-accent)" }}>
                    VibeTerm
                  </div>
                  <h1 style={{ margin: "4px 0 6px 0", "font-size": "24px", "font-weight": 700, color: "var(--color-text)", "line-height": 1.15 }}>
                    {t("stats.subtitle")}
                  </h1>
                  <div style={{ "font-size": "12px", color: "var(--color-text-2)" }}>
                    {t("stats.meta", { n: days(), date: localDate(stats()!.generated_at_ms) })}
                  </div>
                </div>

                <KpiRow stats={stats()!} />

                <Block title={t("stats.daily.title")}>
                  <DailyChart daily={stats()!.daily} rangeDays={days()} />
                </Block>

                <Block title={t("stats.by_model.title")}>
                  <ModelShare rows={stats()!.by_model} />
                </Block>

                <Block title={t("stats.by_project.title")}>
                  <ProjectBars rows={stats()!.by_project} reveal={reveal()} />
                </Block>

                {/* 水印 — 社媒分享归属 */}
                <div style={{ "text-align": "center", "font-size": "11px", color: "var(--color-text-2)", opacity: 0.75, "margin-top": "20px", "padding-top": "14px", "border-top": "1px solid var(--color-border)" }}>
                  {t("stats.generated_by")} · {localDate(stats()!.generated_at_ms)}
                </div>
              </div>
            </Show>
          </Show>
        </div>
      </div>
    </div>
  );
};

// ---- 骨架屏(首扫加载占位)----
/** 单个扫光占位块. */
const Sk: Component<{ w?: string; h: string; r?: string }> = (p) => (
  <div
    style={{
      width: p.w ?? "100%",
      height: p.h,
      "border-radius": p.r ?? "6px",
      background: SHIMMER,
      "background-size": "200% 100%",
      animation: "vt-shimmer 1.6s linear infinite",
    }}
  />
);

const Gap: Component<{ h: string }> = (p) => <div style={{ height: p.h }} />;

/** 与真实面板同构的加载骨架 —— hero / 4 KPI / 趋势柱 / 环形图 / 项目条. */
const StatsSkeleton: Component = () => (
  <div
    data-testid="stats-loading"
    aria-busy="true"
    aria-label={t("stats.loading")}
    style={{ background: "var(--color-surface)", border: "1px solid var(--color-border)", "border-radius": "14px", padding: "26px 28px" }}
  >
    <style>{SHIMMER_CSS}</style>
    {/* hero */}
    <div style={{ "border-bottom": "1px solid var(--color-border)", "padding-bottom": "16px", "margin-bottom": "18px" }}>
      <Sk w="64px" h="11px" />
      <Gap h="8px" />
      <Sk w="260px" h="24px" />
      <Gap h="8px" />
      <Sk w="190px" h="12px" />
    </div>
    {/* KPIs */}
    <div style={{ display: "grid", "grid-template-columns": "repeat(4, 1fr)", gap: "12px", "margin-bottom": "22px" }}>
      <For each={[0, 1, 2, 3]}>
        {() => (
          <div style={{ background: "var(--color-bg)", border: "1px solid var(--color-border)", "border-radius": "10px", padding: "14px 16px" }}>
            <Sk w="60%" h="11px" />
            <Gap h="8px" />
            <Sk w="80%" h="24px" />
          </div>
        )}
      </For>
    </div>
    {/* 每日趋势 */}
    <Sk w="90px" h="13px" />
    <Gap h="12px" />
    <div style={{ ...cardInnerStyle(), height: "196px", display: "flex", "align-items": "flex-end", gap: "3px" }}>
      <For each={SK_BARS}>
        {(h) => (
          <div
            style={{
              flex: "1",
              height: `${h}%`,
              "max-width": "24px",
              margin: "0 auto",
              background: SHIMMER,
              "background-size": "200% 100%",
              animation: "vt-shimmer 1.6s linear infinite",
              "border-radius": "4px 4px 0 0",
            }}
          />
        )}
      </For>
    </div>
    <Gap h="22px" />
    {/* 模型占比: donut + 条 */}
    <Sk w="90px" h="13px" />
    <Gap h="12px" />
    <div style={{ ...cardInnerStyle(), display: "grid", "grid-template-columns": "150px 1fr", gap: "20px", "align-items": "center" }}>
      <div style={{ "justify-self": "center" }}>
        <Sk w="120px" h="120px" r="50%" />
      </div>
      <div style={{ display: "flex", "flex-direction": "column", gap: "12px" }}>
        <For each={[0, 1, 2, 3]}>{() => <Sk h="14px" />}</For>
      </div>
    </div>
    <Gap h="22px" />
    {/* 按项目 */}
    <Sk w="90px" h="13px" />
    <Gap h="12px" />
    <div style={{ ...cardInnerStyle(), display: "flex", "flex-direction": "column", gap: "10px" }}>
      <For each={[0, 1, 2, 3, 4, 5]}>{() => <Sk h="16px" />}</For>
    </div>
  </div>
);

// ---- KPI 行 ----
const KpiRow: Component<{ stats: UsageStats }> = (p) => {
  const totalTokens = () => p.stats.totals.claude_tokens + p.stats.totals.codex_tokens;
  return (
    <div style={{ display: "grid", "grid-template-columns": "repeat(4, 1fr)", gap: "12px", "margin-bottom": "22px" }}>
      <Kpi testid="stats-kpi-tokens" icon={<Hash size={15} />} label={t("stats.kpi.tokens")} value={fmtTokens(totalTokens())} />
      <Kpi
        testid="stats-kpi-cost"
        icon={<Coins size={15} />}
        label={t("stats.kpi.cost")}
        value={fmtCost(p.stats.totals.cost_usd)}
        caveat={p.stats.totals.cost_unknown_entries > 0 ? t("stats.cost_caveat", { n: p.stats.totals.cost_unknown_entries }) : undefined}
      />
      <Kpi testid="stats-kpi-messages" icon={<MessageSquare size={15} />} label={t("stats.kpi.messages")} value={fmtTokens(p.stats.totals.message_count)} />
      <Kpi testid="stats-kpi-models" icon={<Cpu size={15} />} label={t("stats.kpi.models")} value={String(p.stats.by_model.length)} />
    </div>
  );
};

const Kpi: Component<{ label: string; value: string; icon: JSX.Element; caveat?: string; testid?: string }> = (p) => (
  <div
    data-testid={p.testid}
    style={{
      background: "var(--color-bg)",
      border: "1px solid var(--color-border)",
      "border-radius": "10px",
      padding: "14px 16px",
    }}
  >
    <div style={{ display: "flex", "align-items": "center", gap: "5px", color: "var(--color-text-2)", "margin-bottom": "6px" }}>
      <span style={{ color: "var(--color-accent)", display: "inline-flex" }}>{p.icon}</span>
      <span style={{ "font-size": "11px" }}>{p.label}</span>
    </div>
    <div style={{ "font-size": "25px", "font-weight": 700, color: "var(--color-text)", "line-height": 1.1, "font-variant-numeric": "tabular-nums" }}>
      {p.value}
    </div>
    <Show when={p.caveat}>
      <div style={{ "font-size": "10px", color: "var(--color-status-stalled, #d97757)", "margin-top": "4px" }}>{p.caveat}</div>
    </Show>
  </div>
);

// ---- 区块容器 ----
const Block: ParentComponent<{ title: string }> = (p) => (
  <div style={{ "margin-bottom": "22px" }}>
    <h3 style={{ margin: "0 0 12px 0", "font-size": "13px", "font-weight": 600, color: "var(--color-text)", "letter-spacing": "0.01em" }}>
      {p.title}
    </h3>
    {p.children}
  </div>
);

// ---- 每日趋势(Claude + Codex 堆叠柱)----
const DailyChart: Component<{ daily: UsageDailyStat[]; rangeDays: number }> = (p) => {
  // 按自然日铺满 rangeDays 个日格(无数据的天留空 0 高)—— 90 天就真有 90 格,
  // 稀疏的早期一眼可见, 不再把"有数据的天"挤成跟 30 天一样.
  const bars = createMemo<UsageDailyStat[]>(() => {
    const map = new Map(p.daily.map((d) => [d.date, d]));
    const today = new Date();
    const out: UsageDailyStat[] = [];
    for (let i = p.rangeDays - 1; i >= 0; i--) {
      const dt = new Date(today.getFullYear(), today.getMonth(), today.getDate() - i);
      const key = ymdLocal(dt);
      out.push(map.get(key) ?? { date: key, claude_tokens: 0, codex_tokens: 0, cost_usd: null });
    }
    return out;
  });
  const max = () => Math.max(1, ...bars().map((d) => d.claude_tokens + d.codex_tokens));
  // 约 15 个标签上限: 7→每天, 30→隔天(每 2), 90→约每周(每 6). 避免日期挤成一团.
  const labelStep = () => Math.max(1, Math.round(bars().length / 15));
  // 悬停柱子的详情浮层 (跟随光标). null = 未悬停.
  const [hover, setHover] = createSignal<{ x: number; y: number; d: UsageDailyStat } | null>(null);
  return (
    <div style={cardInnerStyle()}>
      <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", "margin-bottom": "10px" }}>
        <div style={{ display: "flex", gap: "14px", "font-size": "11px", color: "var(--color-text-2)" }}>
          <Legend color={CLAUDE_COLOR} label={t("stats.legend.claude")} />
          <Legend color={CODEX_COLOR} label={t("stats.legend.codex")} />
        </div>
        <span style={{ "font-size": "10px", color: "var(--color-text-2)" }}>↑ {fmtTokens(max())}</span>
      </div>
      {/* 柱子区 —— 只画柱子, 基线统一干净 (日期不混进来) */}
      <div style={{ display: "flex", "align-items": "flex-end", gap: "2px", height: "150px", "border-top": "1px dashed var(--color-border)", "padding-top": "4px" }}>
        <For each={bars()}>
          {(d) => {
            const total = () => d.claude_tokens + d.codex_tokens;
            const h = () => (total() / max()) * 100;
            const claudeFrac = () => (total() > 0 ? (d.claude_tokens / total()) * 100 : 0);
            return (
              <div
                data-testid={`stats-daily-${d.date}`}
                onMouseMove={(e) => setHover({ x: e.clientX, y: e.clientY, d })}
                onMouseLeave={() => setHover(null)}
                style={{ flex: "1", display: "flex", "flex-direction": "column", "align-items": "center", "min-width": 0, height: "100%", "justify-content": "flex-end", cursor: "pointer" }}
              >
                <div
                  style={{
                    width: "100%",
                    "max-width": "24px",
                    height: `${h()}%`,
                    "min-height": total() > 0 ? "3px" : "0",
                    display: "flex",
                    "flex-direction": "column",
                    "border-radius": "4px 4px 0 0",
                    overflow: "hidden",
                  }}
                >
                  <div style={{ height: `${100 - claudeFrac()}%`, background: CODEX_COLOR }} />
                  <div style={{ height: `${claudeFrac()}%`, background: CLAUDE_COLOR }} />
                </div>
              </div>
            );
          }}
        </For>
      </div>
      {/* x 轴日期 —— 单独一行, 与柱子用同样的 flex 槽对齐, 每 labelStep 个显示一个 */}
      <div style={{ display: "flex", gap: "2px", "margin-top": "6px" }}>
        <For each={bars()}>
          {(d, i) => (
            <div style={{ flex: "1", "min-width": 0, "text-align": "center" }}>
              <Show when={i() % labelStep() === 0}>
                <span style={{ "font-size": "8px", color: "var(--color-text-2)", "white-space": "nowrap" }}>
                  {shortDate(d.date)}
                </span>
              </Show>
            </div>
          )}
        </For>
      </div>
      {/* 悬停详情浮层 —— 跟随光标, 显示日期 + Claude/Codex token + 成本 */}
      <Show when={hover()}>
        <div
          style={{
            position: "fixed",
            left: `${hover()!.x + 14}px`,
            top: `${hover()!.y + 14}px`,
            "z-index": 3000,
            "pointer-events": "none",
            background: "var(--color-surface)",
            border: "1px solid var(--color-border)",
            "border-radius": "8px",
            padding: "8px 10px",
            "box-shadow": "0 6px 20px rgba(0,0,0,0.45)",
            "font-size": "11px",
            "min-width": "152px",
          }}
        >
          <div style={{ "font-weight": 700, color: "var(--color-text)", "margin-bottom": "5px" }}>{hover()!.d.date}</div>
          <TipRow color={CLAUDE_COLOR} label={t("stats.legend.claude")} val={fmtTokens(hover()!.d.claude_tokens)} />
          <TipRow color={CODEX_COLOR} label={t("stats.legend.codex")} val={fmtTokens(hover()!.d.codex_tokens)} />
          <div style={{ display: "flex", "justify-content": "space-between", "margin-top": "5px", "padding-top": "5px", "border-top": "1px solid var(--color-border)", color: "var(--color-text-2)" }}>
            <span>{t("stats.col.cost")}</span>
            <span style={{ color: "var(--color-text)", "font-variant-numeric": "tabular-nums" }}>{fmtCost(hover()!.d.cost_usd)}</span>
          </div>
        </div>
      </Show>
    </div>
  );
};

const TipRow: Component<{ color: string; label: string; val: string }> = (p) => (
  <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", gap: "16px", margin: "2px 0" }}>
    <span style={{ display: "inline-flex", "align-items": "center", gap: "5px", color: "var(--color-text-2)" }}>
      <span style={{ width: "8px", height: "8px", "border-radius": "2px", background: p.color }} />
      {p.label}
    </span>
    <span style={{ color: "var(--color-text)", "font-variant-numeric": "tabular-nums" }}>{p.val}</span>
  </div>
);

const Legend: Component<{ color: string; label: string }> = (p) => (
  <span style={{ display: "inline-flex", "align-items": "center", gap: "5px" }}>
    <span style={{ width: "9px", height: "9px", background: p.color, "border-radius": "2px", display: "inline-block" }} />
    {p.label}
  </span>
);

// ---- 模型占比:环形图(SVG donut)+ 明细条 ----
const ModelShare: Component<{ rows: UsageModelStat[] }> = (p) => {
  const total = () => Math.max(1, p.rows.reduce((s, r) => s + r.total_tokens, 0));
  const R = 52;
  const C = 2 * Math.PI * R;
  // 累积偏移 → 每段 dasharray/offset.
  const segments = () => {
    let acc = 0;
    return p.rows.map((r, i) => {
      const frac = r.total_tokens / total();
      const len = frac * C;
      const seg = { color: SERIES[i % SERIES.length], len, offset: -acc, frac, row: r };
      acc += len;
      return seg;
    });
  };
  return (
    <div style={{ ...cardInnerStyle(), display: "grid", "grid-template-columns": "150px 1fr", gap: "20px", "align-items": "center" }}>
      {/* donut */}
      <div style={{ position: "relative", width: "140px", height: "140px", "justify-self": "center" }}>
        <svg width="140" height="140" viewBox="0 0 140 140" style={{ transform: "rotate(-90deg)" }}>
          <circle cx="70" cy="70" r={R} fill="none" stroke="var(--color-bg)" stroke-width="18" />
          <For each={segments()}>
            {(s) => (
              <circle
                cx="70"
                cy="70"
                r={R}
                fill="none"
                stroke={s.color}
                stroke-width="18"
                stroke-dasharray={`${Math.max(0, s.len - 1)} ${C}`}
                stroke-dashoffset={s.offset}
              />
            )}
          </For>
        </svg>
        <div style={{ position: "absolute", inset: 0, display: "flex", "flex-direction": "column", "align-items": "center", "justify-content": "center" }}>
          <span style={{ "font-size": "18px", "font-weight": 700, color: "var(--color-text)", "font-variant-numeric": "tabular-nums" }}>
            {fmtTokens(total())}
          </span>
          <span style={{ "font-size": "10px", color: "var(--color-text-2)" }}>{t("stats.col.tokens")}</span>
        </div>
      </div>
      {/* 明细条 */}
      <div style={{ display: "flex", "flex-direction": "column", gap: "8px" }}>
        <For each={segments()}>
          {(s) => (
            <div data-testid={`stats-model-${tid(s.row.model)}`} style={{ "font-size": "12px" }}>
              <div style={{ display: "flex", "align-items": "center", gap: "7px", "margin-bottom": "3px" }}>
                <span style={{ width: "9px", height: "9px", "border-radius": "2px", background: s.color, "flex-shrink": 0 }} />
                <span style={{ color: "var(--color-text)", "white-space": "nowrap", overflow: "hidden", "text-overflow": "ellipsis", flex: 1 }}>{s.row.model}</span>
                <span style={{ color: "var(--color-text)", "font-weight": 600, "font-variant-numeric": "tabular-nums" }}>{fmtPct(s.frac)}</span>
                <span style={{ color: "var(--color-text-2)", "font-variant-numeric": "tabular-nums", "min-width": "54px", "text-align": "right" }}>
                  {fmtTokens(s.row.total_tokens)}
                </span>
                <span style={{ color: "var(--color-text-2)", "font-variant-numeric": "tabular-nums", "min-width": "60px", "text-align": "right" }}>
                  {s.row.cost_usd == null ? t("stats.no_cost") : fmtCost(s.row.cost_usd)}
                </span>
              </div>
              <div style={{ background: "var(--color-bg)", "border-radius": "3px", height: "6px", overflow: "hidden" }}>
                <div style={{ width: `${Math.max(1, s.frac * 100)}%`, height: "100%", background: s.color }} />
              </div>
            </div>
          )}
        </For>
      </div>
    </div>
  );
};

// ---- 按项目水平条(路径可模糊)----
const ProjectBars: Component<{ rows: UsageProjectStat[]; reveal: boolean }> = (p) => {
  const max = () => Math.max(1, ...p.rows.map((r) => r.total_tokens));
  return (
    <div style={{ ...cardInnerStyle(), display: "flex", "flex-direction": "column", gap: "9px" }}>
      <For each={p.rows}>
        {(r, i) => {
          const short = r.project_path.split("/").filter(Boolean).pop() || r.project_path;
          return (
            <div
              data-testid={`stats-project-${tid(r.project_path)}`}
              style={{ display: "grid", "grid-template-columns": "18px 160px 1fr 132px", gap: "10px", "align-items": "center", "font-size": "12px" }}
            >
              <span style={{ color: "var(--color-text-2)", "font-variant-numeric": "tabular-nums", width: "16px", "text-align": "right" }}>{i() + 1}</span>
              <span
                title={p.reveal ? r.project_path : undefined}
                style={{
                  color: "var(--color-text)",
                  "white-space": "nowrap",
                  overflow: "hidden",
                  "text-overflow": "ellipsis",
                  filter: p.reveal ? "none" : "blur(6px)",
                  "user-select": p.reveal ? "auto" : "none",
                  transition: "filter 120ms ease",
                }}
              >
                {short}
              </span>
              <div style={{ background: "var(--color-bg)", "border-radius": "4px", height: "16px", overflow: "hidden" }}>
                <div style={{ width: `${Math.max(2, (r.total_tokens / max()) * 100)}%`, height: "100%", background: SERIES[i() % SERIES.length], "border-radius": "4px 0 0 4px" }} />
              </div>
              <span style={{ color: "var(--color-text-2)", "white-space": "nowrap", "font-variant-numeric": "tabular-nums", "text-align": "right", overflow: "hidden", "text-overflow": "ellipsis" }}>
                {fmtTokens(r.total_tokens)}
                <Show when={r.cost_usd != null} fallback={<span style={{ opacity: 0.5 }}> · {t("stats.no_cost")}</span>}>
                  <span> · {fmtCost(r.cost_usd)}</span>
                </Show>
              </span>
            </div>
          );
        }}
      </For>
    </div>
  );
};

// ---- 共享样式 ----
function cardInnerStyle() {
  return {
    background: "var(--color-bg)",
    border: "1px solid var(--color-border)",
    "border-radius": "10px",
    padding: "14px 16px",
  };
}
function backBtnStyle() {
  return {
    display: "inline-flex",
    "align-items": "center",
    gap: "4px",
    padding: "3px 9px",
    background: "transparent",
    color: "var(--color-text-2)",
    border: "1px solid var(--color-border)",
    "border-radius": "5px",
    cursor: "pointer",
    "font-size": "11px",
    "font-weight": 500,
  };
}
function pillStyle(active: boolean) {
  return {
    background: active ? "var(--color-accent)" : "transparent",
    color: active ? "var(--color-bg)" : "var(--color-text-2)",
    border: "1px solid var(--color-border)",
    "border-radius": "4px",
    padding: "3px 10px",
    cursor: "pointer",
    "font-size": "11px",
  };
}
function toolBtnStyle(primary = false) {
  return {
    display: "inline-flex",
    "align-items": "center",
    gap: "5px",
    padding: "4px 10px",
    background: primary ? "var(--color-accent)" : "transparent",
    color: primary ? "var(--color-bg)" : "var(--color-text-2)",
    border: "1px solid " + (primary ? "var(--color-accent)" : "var(--color-border)"),
    "border-radius": "5px",
    cursor: "pointer",
    "font-size": "11px",
    "font-weight": 500,
  };
}
function emptyStyle() {
  return { padding: "60px 40px", "text-align": "center" as const, color: "var(--color-text-2)", "font-size": "13px" };
}
