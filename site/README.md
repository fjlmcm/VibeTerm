# VibeTerm 官网

VibeTerm 产品官网 —— 纯静态、多语言、多主题、酷炫动画。

- **框架**:Astro 5(静态输出,默认零运行时 JS,交互处局部水合)
- **语言**:English(根) · 简体中文(`/zh`) · 日本語(`/ja`)
- **主题**:复用产品内置的 **10 套终端主题**,点击全站实时换肤(单一数据源 `src/data/themes.ts`)
- **动画**:GSAP + ScrollTrigger(滚动进场)· Lenis(平滑滚动)· canvas 节点连线 · braille spinner;全程 `prefers-reduced-motion` 降级
- **设计**:终端原生 × 编辑感(等宽排版 · CRT 微辉光 · 节点母题 · 命令行交互)

## 开发

独立 npm 项目,**不在根 pnpm workspace 内**(避免污染主 app 的 lock/构建):

```bash
cd site
pnpm install --ignore-workspace
pnpm dev        # http://localhost:4321/
pnpm build      # 产出 dist/
pnpm check      # astro 类型检查
```

## 部署

`.github/workflows/deploy-site.yml` 在 `site/**` 改动 push 到 main 时自动构建并发布到 GitHub Pages。

**首次启用**:仓库 Settings → Pages → Source 选 **GitHub Actions**,Custom domain 填 `www.vibeterm.org`,勾选 Enforce HTTPS。
站点地址:`https://www.vibeterm.org/`(自定义域名,根路径部署 → `astro.config.mjs` 无 `base`;`site/public/CNAME` 已含域名)。

**DNS**(域名商处):给 `www.vibeterm.org` 加一条 **CNAME** 记录 → 指向 `fjlmcm.github.io`。

> 若改回 `fjlmcm.github.io/VibeTerm/` 子路径,需把 `astro.config.mjs` 的 `base` 设回 `/VibeTerm` 并删除 CNAME。

## 结构

```
src/
├── data/        # 主题配色 / 特性 / CJK 对比 / 致谢(从产品移植,事实数据)
├── i18n/        # en / zh / ja 三语字典 + t() helper
├── styles/      # tokens(全局 token)+ global(reset / 工具 / 氛围)
├── lib/         # theme(主题切换)· motion(GSAP+Lenis 编排)· site(常量)
├── components/  # ThemeStyles · Nav · Footer · Landing · hero/ · sections/
├── layouts/     # Base(SEO / 字体 / 防 FOUC / 挂载)
└── pages/       # index(en) · zh/ · ja/
```

文案改动集中在 `src/i18n/{en,zh,ja}.ts`(三语 key 严格对齐);主题改动改 `src/data/themes.ts` 即全站生效。
