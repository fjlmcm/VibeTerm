// @ts-check
import { defineConfig } from 'astro/config';
import { readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// 自动发现 locales:src/i18n/locales/*.json → 语言列表。
// 加语言 = 丢一个 json,无需改这里。
const localesDir = fileURLToPath(new URL('./src/i18n/locales', import.meta.url));
const locales = readdirSync(localesDir)
  .filter((f) => f.endsWith('.json'))
  .map((f) => f.replace('.json', ''));

// VibeTerm 官网 —— 纯静态,部署到 GitHub Pages + 自定义域名
// https://www.vibeterm.org/(根路径,无需 base)
export default defineConfig({
  site: 'https://www.vibeterm.org',
  trailingSlash: 'ignore',
  i18n: {
    locales,
    defaultLocale: 'en',
    routing: {
      prefixDefaultLocale: false,
      redirectToDefaultLocale: false,
    },
  },
  build: {
    inlineStylesheets: 'auto',
  },
});
