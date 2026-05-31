// 校验 app i18n 各 locale 与 en(权威源)对齐:key 不缺不多。
// 用法: node scripts/check-i18n.mjs   (在 @vibeterm/ui-core 下)
import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const dir = join(here, "..", "src", "i18n", "locales");
const SOURCE = "en";

const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
const dicts = {};
for (const f of files) {
  dicts[f.replace(".json", "")] = JSON.parse(readFileSync(join(dir, f), "utf8"));
}

if (!dicts[SOURCE]) {
  console.error(`✗ source locale ${SOURCE}.json not found`);
  process.exit(1);
}

const enKeys = Object.keys(dicts[SOURCE]).sort();
const enSet = new Set(enKeys);
let bad = 0;

for (const loc of Object.keys(dicts).sort()) {
  const keys = Object.keys(dicts[loc]);
  const kset = new Set(keys);
  const missing = enKeys.filter((k) => !kset.has(k));
  const extra = keys.filter((k) => !enSet.has(k));
  if (missing.length || extra.length) {
    bad++;
    console.error(`✗ [${loc}] missing=${missing.length} extra=${extra.length}`);
    if (missing.length)
      console.error(`    missing: ${missing.slice(0, 10).join(", ")}${missing.length > 10 ? " …" : ""}`);
    if (extra.length)
      console.error(`    extra: ${extra.slice(0, 10).join(", ")}${extra.length > 10 ? " …" : ""}`);
  } else {
    console.log(`✓ [${loc}] ${keys.length} keys`);
  }
}

if (bad) {
  console.error(`\n✗ ${bad} locale(s) out of sync with ${SOURCE}`);
  process.exit(1);
}
console.log(`\nall ${Object.keys(dicts).length} locales aligned (${enKeys.length} keys)`);
