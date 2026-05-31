// i18n 核心 —— 自动发现 locales/*.json,en 为权威,缺 key 回退英文。
// 加语言 = 往 locales/ 丢一个 json + 在 META 补一行显示名,不改其他代码。
const modules = import.meta.glob<Record<string, string>>('./locales/*.json', {
  eager: true,
  import: 'default',
});

const localeOf = (path: string): string =>
  path.slice(path.lastIndexOf('/') + 1).replace('.json', '');

const dictionaries: Record<string, Record<string, string>> = {};
for (const [path, dict] of Object.entries(modules)) {
  dictionaries[localeOf(path)] = dict;
}

export type Locale = string;
export const DEFAULT_LOCALE: Locale = 'en';

// 语言显示名 + <html lang>。加语言补一行;缺省回退 locale 代号本身。
const META: Record<string, { name: string; html: string }> = {
  en: { name: 'English', html: 'en' },
  zh: { name: '简体中文', html: 'zh-Hans' },
  ja: { name: '日本語', html: 'ja' },
  ko: { name: '한국어', html: 'ko' },
  'zh-hant': { name: '繁體中文', html: 'zh-Hant' },
  es: { name: 'Español', html: 'es' },
  'pt-br': { name: 'Português', html: 'pt-BR' },
  de: { name: 'Deutsch', html: 'de' },
  fr: { name: 'Français', html: 'fr' },
  ru: { name: 'Русский', html: 'ru' },
  it: { name: 'Italiano', html: 'it' },
  tr: { name: 'Türkçe', html: 'tr' },
  vi: { name: 'Tiếng Việt', html: 'vi' },
  id: { name: 'Bahasa Indonesia', html: 'id' },
};

// 排序:常用语种优先,其余字母序
const PREFERRED = ['en', 'zh', 'zh-hant', 'ja', 'ko', 'vi', 'id', 'es', 'pt-br', 'de', 'fr', 'it', 'ru', 'tr'];
export const LOCALES: Locale[] = Object.keys(dictionaries).sort((a, b) => {
  const ia = PREFERRED.indexOf(a);
  const ib = PREFERRED.indexOf(b);
  if (ia === -1 && ib === -1) return a.localeCompare(b);
  return (ia === -1 ? 999 : ia) - (ib === -1 ? 999 : ib);
});

export const LOCALE_NAMES: Record<string, string> = Object.fromEntries(
  LOCALES.map((l) => [l, META[l]?.name ?? l]),
);
export const HTML_LANG: Record<string, string> = Object.fromEntries(
  LOCALES.map((l) => [l, META[l]?.html ?? l]),
);

export function isLocale(value: string): boolean {
  return value in dictionaries;
}

/** 返回绑定到某语言的 t();缺 key 回退英文,再回退 key 本身 */
export function useTranslations(locale: string) {
  const dict = dictionaries[locale] ?? dictionaries[DEFAULT_LOCALE];
  const fallback = dictionaries[DEFAULT_LOCALE];
  return function t(key: string, params?: Record<string, string | number>): string {
    let value = dict[key] ?? fallback[key] ?? key;
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        value = value.replaceAll(`{${k}}`, String(v));
      }
    }
    return value;
  };
}

/** 从含 base 的路径里识别 locale(Astro.currentLocale 的兜底) */
export function localeFromPath(pathname: string): Locale {
  for (const part of pathname.split('/').filter(Boolean)) {
    if (isLocale(part)) return part;
  }
  return DEFAULT_LOCALE;
}
