// i18n key 对齐校验:以 en.json 为基准,报告各语言缺失 / 多余 key。
// 用法: pnpm check:i18n   (CI 里跑;有问题退出码 1)
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const dir = fileURLToPath(new URL('../src/i18n/locales/', import.meta.url));
const files = readdirSync(dir).filter((f) => f.endsWith('.json'));
const dicts = Object.fromEntries(
  files.map((f) => [f.replace('.json', ''), JSON.parse(readFileSync(dir + f, 'utf8'))]),
);

const base = dicts.en;
if (!base) {
  console.error('✗ missing en.json (source of truth)');
  process.exit(1);
}
const baseKeys = Object.keys(base);
let problems = 0;

for (const loc of Object.keys(dicts).sort()) {
  const dict = dicts[loc];
  const keys = new Set(Object.keys(dict));
  const missing = baseKeys.filter((k) => !keys.has(k));
  const extra = Object.keys(dict).filter((k) => !(k in base));
  if (missing.length || extra.length) {
    problems++;
    console.log(`✗ [${loc}] ${Object.keys(dict).length}/${baseKeys.length} keys`);
    if (missing.length) console.log(`    missing (${missing.length}): ${missing.join(', ')}`);
    if (extra.length) console.log(`    extra (${extra.length}): ${extra.join(', ')}`);
  } else {
    console.log(`✓ [${loc}] ${baseKeys.length} keys`);
  }
}

if (problems) {
  console.error(`\n${problems} locale(s) out of sync with en.json`);
  process.exit(1);
}
console.log(`\nall ${Object.keys(dicts).length} locales aligned (${baseKeys.length} keys)`);
