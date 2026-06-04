# 规则:i18n 完整性(HIGH)

新增任何用户可见文案,必须覆盖全部 locale,否则运行时静默 fallback 英文。

## 审查要点

- 前端新增 i18n key:必须同时加到 `web/packages/ui-core/src/i18n/locales/` 下**全部 14 个** locale,
  以 `en.json` 为权威。CI 的 `scripts/check-i18n.mjs` 会校验;PR 若只改 en/zh-CN 不补其余,标 HIGH。
- 带 `{placeholder}` 的值:各 locale 的占位符集合必须与 en 一致(漏写 `{pct}` 之类会运行时显示错误)。
- **新增一门语言**:必须三处同步 —— ① 前端 `LANG_META`(`i18n/index.ts`)② Rust 顶栏菜单 `MenuLang`
  ③ Rust 菜单标签 `LBL`。漏 Rust 那两处会导致顶栏菜单 fallback 英文(本仓踩过)。

## 严重级

漏补 locale / 占位符不一致 → HIGH。加语言漏 Rust 菜单同步 → HIGH。
