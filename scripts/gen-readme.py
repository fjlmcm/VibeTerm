#!/usr/bin/env python3
"""生成 14 语 README —— 单一数据源 = 官网 i18n locales。

README 的营销文案(标语 / 价值主张 / 原则 / 5 状态 / 特性 / 机制 / CJK / 致谢 / 下载)
与官网 site/src/i18n/locales/*.json 一一对应。这里读那 14 份 JSON,套同一个 markdown
模板渲染出 README.md(en,GitHub 默认)+ README.<code>.md(其余 13 语)。

为何用生成器而非手写 14 份:① 译文已是官网专业翻译,直接复用、不引入我的翻译质量风险;
② 14 份保持结构一致;③ 官网文案改了重跑即可同步。代码块 / 链接 / 第三方项目名等通用技术
内容保持英文(各语言 README 通行做法)。

用法:python3 scripts/gen-readme.py   # 写出 README.md + README.<code>.md
"""
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
LOC_DIR = ROOT / "site" / "src" / "i18n" / "locales"

# 显示顺序(常用语种优先)+ 各语言自称(与官网 META 一致)
ORDER = ["en", "zh", "zh-hant", "ja", "ko", "vi", "id", "es", "pt-br", "de", "fr", "it", "ru", "tr"]
NAME = {
    "en": "English", "zh": "简体中文", "zh-hant": "繁體中文", "ja": "日本語",
    "ko": "한국어", "vi": "Tiếng Việt", "id": "Bahasa Indonesia", "es": "Español",
    "pt-br": "Português", "de": "Deutsch", "fr": "Français", "it": "Italiano",
    "ru": "Русский", "tr": "Türkçe",
}

REPO = "https://github.com/fjlmcm/VibeTerm"
SITE = "https://www.vibeterm.org"

# 词间无空格的语种:标语 hero.title 换行拼单行时不加空格(全角逗号后留空格很违和)。
# 仅中文 + 日文;韩语(ko)虽属 CJK 但词间用空格,逗号后需空格,故不在内。
NO_WORD_SPACE = {"zh", "zh-hant", "ja"}

dicts = {p.stem: json.loads(p.read_text("utf-8")) for p in LOC_DIR.glob("*.json")}
EN = dicts["en"]


def fname(loc: str) -> str:
    return "README.md" if loc == "en" else f"README.{loc}.md"


def lang_switcher(cur: str) -> str:
    parts = []
    for loc in ORDER:
        if loc not in dicts:
            continue
        parts.append(NAME[loc] if loc == cur else f"[{NAME[loc]}]({fname(loc)})")
    # 当前语言加粗
    parts = [f"**{p}**" if p == NAME[cur] else p for p in parts]
    return " · ".join(parts)


def render(loc: str) -> str:
    d = dicts[loc]

    def t(key: str) -> str:
        return d.get(key, EN.get(key, key))

    tagline = t("hero.title").replace("\n", "" if loc in NO_WORD_SPACE else " ")
    L = []
    A = L.append

    # ---- 居中头部:hero 图 + 标题 + 标语 + 价值主张 + badge + 链接 + 语言切换 ----
    A('<div align="center">')
    A("")
    A(f'<img src="docs/hero.png" alt="VibeTerm" width="820">')
    A("")
    A("# VibeTerm")
    A("")
    A(f"**{tagline}**")
    A("")
    A(t("hero.subtitle"))
    A("")
    A(f"[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)")
    A(f"[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)]({REPO}/releases)")
    A(f"[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)]({REPO}/releases)")
    A(f"[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)]({REPO})")
    A("")
    A(f"[**{SITE.split('//')[1]}**]({SITE}) · [**{t('download.macos')}**]({REPO}/releases) · [**GitHub**]({REPO})")
    A("")
    A(lang_switcher(loc))
    A("")
    A("</div>")
    A("")
    A("---")
    A("")

    # ---- 它有何不同(6 原则)----
    A(f"## {t('section.principles.title')}")
    A("")
    for k in ["zeroIntrusion", "multiAgent", "terminal", "cjk", "privacy", "oss"]:
        A(f"- **{t(f'principle.{k}.title')}** — {t(f'principle.{k}.desc')}")
    A("")

    # ---- 5 状态圆点(配 hero 图)----
    A(f"## {t('mechanism.states.title')}")
    A("")
    dots = {"running": "🔵", "waiting": "🟡", "stalled": "🔴", "idle": "⚪", "done": "🟢"}
    for k, dot in dots.items():
        A(f"- {dot} **{t(f'states.{k}.label')}** — {t(f'states.{k}.desc')}")
    A("")

    # ---- 特性(三组)----
    A(f"## {t('section.features.title')}")
    A("")
    A(f"_{t('section.features.subtitle')}_")
    A("")
    groups = {
        "agent": ["sniff", "urgency", "monitor", "stats"],
        "terminal": ["split", "canvas", "floating", "render"],
        "productivity": ["palette", "prompts", "statusbar", "notify", "theme"],
    }
    for g, feats in groups.items():
        A(f"### {t(f'features.group.{g}')}")
        A("")
        for f in feats:
            A(f"- **{t(f'features.{f}.label')}** — {t(f'features.{f}.desc')}")
        A("")

    # ---- 机制:三层嗅探 + 红线 ----
    A(f"## {t('section.mechanism.title')}")
    A("")
    A(t("section.mechanism.subtitle"))
    A("")
    for i in (1, 2, 3):
        A(f"{i}. **{t(f'mechanism.layer{i}.title')}** — {t(f'mechanism.layer{i}.desc')}")
    A("")
    A(f"> **{t('mechanism.redline.title')}** — {t('mechanism.redline.desc')}")
    A("")

    # ---- CJK ----
    A(f"## {t('section.cjk.title')}")
    A("")
    A(t("section.cjk.subtitle"))
    A("")
    for k in ["ime", "width", "wrap", "copy", "render"]:
        A(f"- {t(f'cjk.point.{k}')}")
    A("")

    # ---- 安装(标题用官网 section.download.title,链接/命令通用)----
    A(f"## {t('section.download.title')}")
    A("")
    A(t("download.note"))
    A("")
    A(f"**[{t('download.macos')} →]({REPO}/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.")
    A("")
    A(f"{t('download.build')}:")
    A("")
    A("```bash")
    A("pnpm install")
    A("pnpm build      # = tauri build → src-tauri/target/release/bundle/")
    A("pnpm dev        # dev (Vite HMR + tauri dev)")
    A("```")
    A("")
    A("Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).")
    A("")

    # ---- 致谢 + License ----
    A(f"## {t('section.credits.title')}")
    A("")
    A(t("credits.thanks"))
    A("")
    A(f"Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).")
    A("")
    A(f"## {t('footer.license')}")
    A("")
    A(f"[MIT](LICENSE) · {t('footer.copyright')}")
    A("")

    return "\n".join(L)


def main():
    written = []
    for loc in ORDER:
        if loc not in dicts:
            print(f"  ! 缺 locale: {loc}(跳过)")
            continue
        out = ROOT / fname(loc)
        out.write_text(render(loc), encoding="utf-8")
        written.append(fname(loc))
    print(f"✓ 生成 {len(written)} 份 README:")
    for w in written:
        print(f"    {w}")


if __name__ == "__main__":
    main()
