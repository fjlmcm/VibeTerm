# 发布与自动更新配置

本文档汇总应用内自更新(I1)、nightly 通道(I2)、Homebrew(I3)、自动 code review(I4)、
i18n 完整性 CI(I7)所需的**一次性配置**(借鉴 cmux 的工程成熟度)。代码/配置已就位,
以下是需要你(维护者)在 GitHub 侧补的 secret 与仓库。

---

## I1 · 应用内自动更新(tauri-plugin-updater)

升级了「设置 → 更新」页:发现新版后可直接「下载并安装」(校验 minisign 签名 → 原地更新 → 重启),
失败回退到「打开下载页」。**仍严格用户手动触发,绝不后台自动检查/下载**(零侵入)。

### 已完成(代码侧)
- `tauri.conf.json` 加 `plugins.updater`(endpoint=releases/latest/latest.json,pubkey 已嵌入)。
- `createUpdaterArtifacts` **不在基础配置**,而在 `src-tauri/tauri.updater.conf.json`,仅 CI 用 `--config` 合并开启
  —— 这样本地无密钥的 `pnpm build` 不会因缺签名私钥报错(零回归),CI 才出 updater 产物。
- Cargo 加 `tauri-plugin-updater` / `tauri-plugin-process`;builder 注册;capabilities 放行。
- 前端 `settings-update.tsx` 接 `check()` → `downloadAndInstall()` → `relaunch()`,带进度。
- `release.yml` 加签名 env + `--config tauri.updater.conf.json`;tauri-action 据此签名 updater 产物并生成/上传 `latest.json`。

> 本地若也想产出可自更新的包(非必须,发布走 CI 即可):
> `export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.vibeterm/updater.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""`
> 再 `pnpm tauri build --config tauri.updater.conf.json`。不加则本地包无 updater 产物(但仍可正常分发,自更新由 CI 发布的版本提供)。

### 你要做的(2 步)
签名密钥对已生成在仓库外:`~/.vibeterm/updater.key`(私钥,已 chmod 600)与 `.key.pub`(公钥,已嵌入配置)。

```bash
# 1. 私钥 → GitHub secret(内容是整个私钥文件)
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo fjlmcm/VibeTerm < ~/.vibeterm/updater.key
# 2. 私钥密码(本次生成为空密码)
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo fjlmcm/VibeTerm --body ""
```

> ⚠️ 私钥务必备份(`~/.vibeterm/updater.key`)。丢失则无法再签名更新包,所有已安装客户端将无法自更新
> (只能引导用户重新下载安装新公钥的版本)。想换带密码的密钥:
> `pnpm tauri signer generate -w ~/.vibeterm/updater.key -f`,把新 `.pub` 更新到 `tauri.conf.json` 的 `pubkey`。

### 生效条件
下一个带这套 secret 的 release(tag push 触发 `release.yml`)会上传 `latest.json` 后,
应用内「下载并安装」才可用;在此之前自动回退「打开下载页」,不影响常规分发。

---

## I2 · Nightly 通道

`nightly.yml`:每日(08:00 UTC)或手动 dispatch,从最新 `main` 构建 macOS universal(签名+公证)→
`nightly` prerelease。独立 bundle id `com.vibeterm.desktop.nightly`(`tauri.nightly.conf.json`),与稳定版并存。
无新提交则跳过。无需新 secret(复用 6 个 `APPLE_*`)。Nightly 无应用内自更新(轻量,手动下载)。

下载:<https://github.com/fjlmcm/VibeTerm/releases/tag/nightly>

---

## I3 · Homebrew Cask

cask 模板:`homebrew/Casks/vibeterm.rb`(唯一真相)。`update-homebrew.yml` 在稳定版 Release **成功完成后**
(用 `workflow_run` 而非 `release:published`,避免资产未传完导致 sha256 对不上)自动把版本+sha256 推到 tap 仓。

### 你要做的(一次性)
```bash
# 1. 建 tap 仓(必须 homebrew- 前缀)
gh repo create fjlmcm/homebrew-vibeterm --public

# 2. 配一个对该 tap 仓有 push 权限的 token(PAT 或 fine-grained),存为 secret
gh secret set HOMEBREW_TAP_TOKEN --repo fjlmcm/VibeTerm --body "<token>"
```
之后用户即可:
```bash
brew tap fjlmcm/vibeterm
brew install --cask vibeterm
```

---

## I4 · 自动 Code Review(红线机器化)

`.coderabbit.yaml` + `.github/review-bot-rules/` 把 5 条红线(零侵入 / 配置隔离 / CJK / i18n / 主题不擅改)
+ 无硬编码真实路径,按文件路径作用域落成 CodeRabbit 审查指令。

### 你要做的
在 <https://coderabbit.ai> 用 GitHub 账号授权 `fjlmcm/VibeTerm`(开源仓免费)。之后每个 PR 自动按红线审查。
Greptile 等其它审查器可指向同一份 `.github/review-bot-rules/`。

---

## I7 · i18n 完整性 CI

`scripts/check-i18n.mjs`:以 `en.json` 为权威,校验全部 14 个 locale 的 key 与占位符完全一致,缺则 fail。
已接入 `ci.yml` 的 lint job。本地自查:`node scripts/check-i18n.mjs`。
新增语言记得三处同步:前端 `LANG_META` + Rust `MenuLang` + Rust `LBL`。
