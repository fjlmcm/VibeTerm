// 14 语 i18n —— locales/*.json 自动发现,en 为权威 fallback。
//
// 加语言 = 往 locales/ 丢一个 json + 在 LANG_META 补一行
//   (母语显示名 + navigator.language 匹配前缀)。其余代码不用动。
// 缺 key fallback en;再缺显示 key 本身。响应式:setLang 触发 t() 重渲染。

import { createSignal } from "solid-js";
import { setMenuLang } from "../ipc";

// Vite glob:构建时内联所有 locale JSON(path → default export)
const modules = import.meta.glob<Record<string, string>>("./locales/*.json", {
  eager: true,
  import: "default",
});

const localeOf = (p: string): string =>
  p.slice(p.lastIndexOf("/") + 1).replace(".json", "");

const DICTS: Record<string, Record<string, string>> = {};
for (const [p, dict] of Object.entries(modules)) {
  DICTS[localeOf(p)] = dict;
}

export type Lang = string;
const DEFAULT_LANG = "en";

// 母语显示名 + navigator.language 匹配标签(全小写)。
// match 同时含完整 tag(zh-tw)与主语言(zh);detectInitialLang 先完整 tag 后主语言,
// 这样 zh-TW/zh-HK 落繁体,其余 zh-* 落简体。加语言时补一行即可。
const LANG_META: Record<string, { name: string; match: string[] }> = {
  "zh-CN": { name: "简体中文", match: ["zh-cn", "zh-hans", "zh-sg", "zh-my", "zh"] },
  "zh-Hant": { name: "繁體中文", match: ["zh-tw", "zh-hk", "zh-mo", "zh-hant"] },
  en: { name: "English", match: ["en"] },
  ja: { name: "日本語", match: ["ja"] },
  ko: { name: "한국어", match: ["ko"] },
  vi: { name: "Tiếng Việt", match: ["vi"] },
  id: { name: "Bahasa Indonesia", match: ["id", "in"] },
  es: { name: "Español", match: ["es"] },
  "pt-BR": { name: "Português", match: ["pt-br", "pt"] },
  de: { name: "Deutsch", match: ["de"] },
  fr: { name: "Français", match: ["fr"] },
  it: { name: "Italiano", match: ["it"] },
  ru: { name: "Русский", match: ["ru"] },
  tr: { name: "Türkçe", match: ["tr"] },
};

// 显示顺序:常用优先
const PREFERRED = [
  "zh-CN", "zh-Hant", "en", "ja", "ko", "vi", "id",
  "es", "pt-BR", "de", "fr", "it", "ru", "tr",
];

/** 已加载且有显示名的语言,按 PREFERRED 排序(给语言选择器用) */
export const LANGS: Lang[] = Object.keys(DICTS)
  .filter((l) => l in LANG_META)
  .sort((a, b) => {
    const ia = PREFERRED.indexOf(a);
    const ib = PREFERRED.indexOf(b);
    return (ia === -1 ? 999 : ia) - (ib === -1 ? 999 : ib);
  });

/** locale → 母语显示名 */
export const LANG_NAMES: Record<string, string> = Object.fromEntries(
  LANGS.map((l) => [l, LANG_META[l]?.name ?? l]),
);

const STORAGE_KEY = "vibeterm-lang";

function detectInitialLang(): Lang {
  // 1. 用户手动选择(持久化)优先
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved && DICTS[saved]) return saved;
  } catch {
    // localStorage 不可用(隐私模式等)→ 跳过
  }
  // 2. navigator.language:先完整 tag(zh-tw),后主语言(zh)
  const sys = (navigator.language || "en").toLowerCase();
  const primary = sys.split("-")[0];
  for (const l of PREFERRED) {
    if (DICTS[l] && LANG_META[l]?.match.includes(sys)) return l;
  }
  for (const l of PREFERRED) {
    if (DICTS[l] && LANG_META[l]?.match.includes(primary)) return l;
  }
  return DICTS[DEFAULT_LANG] ? DEFAULT_LANG : LANGS[0] ?? DEFAULT_LANG;
}

const [lang, setLangSignal] = createSignal<Lang>(detectInitialLang());

export const currentLang = lang;

// 同步顶栏菜单语言(仅 macOS 重建 NSMenu;其他平台后端 noop)
// 非 Tauri 上下文 invoke 会 reject,catch 兜底即可
function syncMenuLang(l: Lang) {
  void setMenuLang(l).catch(() => {});
}

// 启动时同步一次 — 让后端 menu_lang 跟前端初始语言一致
syncMenuLang(lang());

export function setLang(l: Lang) {
  setLangSignal(l);
  try {
    localStorage.setItem(STORAGE_KEY, l);
  } catch {
    // 忽略持久化失败
  }
  syncMenuLang(l);
}

/** 翻译 + 简单插值。响应式:语言变化触发重渲染 */
export function t(key: string, params?: Record<string, string | number>): string {
  const l = lang();
  let raw = DICTS[l]?.[key] ?? DICTS[DEFAULT_LANG]?.[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      raw = raw.replace(new RegExp(`\\{${k}\\}`, "g"), () => String(v));
    }
  }
  return raw;
}

/** 查 i18n key, 不存在则返回 fallback (不会返回 key 本身). 用于 prompt preset name. */
export function tOr(key: string, fallback: string): string {
  const l = lang();
  return DICTS[l]?.[key] ?? DICTS[DEFAULT_LANG]?.[key] ?? fallback;
}

/**
 * prompt 显示名 — 先查 i18n key `prompts.preset.<id>.name`,
 * 找不到 fallback 到 PromptEntry.name (用户自定义的不撞 i18n key, 直接走 fallback).
 */
export function promptDisplayName(entry: { id: string; name: string }): string {
  return tOr(`prompts.preset.${entry.id}.name`, entry.name);
}

/**
 * prompt 实际写入 PTY 的内容 — 内置 agent 预设按当前 lang 翻译,
 * terminal 类命令不走翻译 (i18n 字典里也不存 content key, 自然 fallback 到原文).
 */
export function promptDisplayContent(entry: { id: string; content: string }): string {
  return tOr(`prompts.preset.${entry.id}.content`, entry.content);
}
