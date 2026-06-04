#!/usr/bin/env node
// i18n 完整性检查 —— en 为权威 key 集,所有 locale 必须与之完全一致。
//
// 借鉴 cmux 的 "full-internationalization" review 规则:新增任何用户可见文案若没覆盖全部 locale,
// CI 就 fail。治本仓 MEMORY 记的坑:加语言/加文案漏补某 locale → 运行时静默 fallback 英文。
//
// 检查项:
//   1. 每个 locale 是否缺 en 里有的 key(missing)
//   2. 每个 locale 是否有 en 里没有的 key(extra → 说明 en 漏了,也算 drift)
//   3. 带 {placeholder} 的值,占位符集合是否与 en 一致(防翻译时漏写 / 写错占位符)
//
// 用法:node scripts/check-i18n.mjs   (退出码 != 0 表示有 drift)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dir = path.join(root, "web/packages/ui-core/src/i18n/locales");
const SOURCE = "en";

const placeholders = (s) =>
  typeof s === "string" ? new Set([...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1])) : new Set();
const setEq = (a, b) => a.size === b.size && [...a].every((x) => b.has(x));

const files = fs.readdirSync(dir).filter((f) => f.endsWith(".json"));
const load = (loc) => JSON.parse(fs.readFileSync(path.join(dir, `${loc}.json`), "utf8"));

const src = load(SOURCE);
const srcKeys = Object.keys(src);
let problems = 0;

for (const file of files.sort()) {
  const loc = file.replace(/\.json$/, "");
  if (loc === SOURCE) continue;
  const obj = load(loc);
  const keys = new Set(Object.keys(obj));
  const missing = srcKeys.filter((k) => !keys.has(k));
  const extra = Object.keys(obj).filter((k) => !(k in src));
  const phMismatch = srcKeys.filter(
    (k) => keys.has(k) && !setEq(placeholders(src[k]), placeholders(obj[k])),
  );

  if (missing.length || extra.length || phMismatch.length) {
    problems++;
    console.error(`\n✗ ${loc}.json`);
    if (missing.length) console.error(`  缺失 ${missing.length} 个 key(en 有、此 locale 无):\n    ${missing.join("\n    ")}`);
    if (extra.length) console.error(`  多出 ${extra.length} 个 key(en 无 → en 漏补):\n    ${extra.join("\n    ")}`);
    if (phMismatch.length) console.error(`  占位符不一致 ${phMismatch.length} 个 key:\n    ${phMismatch.join("\n    ")}`);
  }
}

if (problems) {
  console.error(`\n❌ i18n 完整性检查失败:${problems} 个 locale 与 ${SOURCE} 不一致。`);
  console.error(`   修复:对照 web/packages/ui-core/src/i18n/locales/${SOURCE}.json 补齐缺失 key。`);
  process.exit(1);
}
console.log(`✓ i18n 完整性 OK —— ${files.length} 个 locale 与 ${SOURCE}(${srcKeys.length} keys)完全一致。`);
