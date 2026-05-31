# Windows 真 Tauri E2E

**为什么 Windows 能免费跑、macOS 不能** —

| 平台 | webview | WebDriver 实现 | 免费? |
|---|---|---|---|
| Windows | WebView2(基于 Chromium) | **msedgedriver.exe**(Microsoft 免费) | ✅ |
| Linux | webkit2gtk | webkit2gtk-driver(apt 装) | ✅ |
| macOS | WKWebView(Apple) | 无公开 driver(Apple 不暴露) | ❌ 需 CrabNebula CN_API_KEY |

**这套文档假设你在 Windows 11 机器上** 把仓库 clone 一份过去跑。`specs/` 同一份测试代码,macOS 上跑 mock 版(Playwright),Windows 上跑真 Tauri 版(WDIO + msedgedriver)。

---

## 一次性 setup

```powershell
# 1. 装 Rust + Node toolchain
winget install Rustlang.Rustup
winget install OpenJS.NodeJS.LTS
winget install pnpm.pnpm

# 2. 装 tauri 系统依赖(WebView2 Runtime — Win11 已自带,Win10 要手动)
# https://developer.microsoft.com/en-us/microsoft-edge/webview2/

# 3. 装 msedgedriver(WebView2 + WebDriver 必需)
# 自动方案:跟系统 Edge 版本匹配
winget install Microsoft.Edge.WebDriver
# 或手动从 https://developer.microsoft.com/en-us/microsoft-edge/tools/webdriver/ 下载,
# 解压到 PATH 里某个目录(eg. C:\tools\),验证:
msedgedriver --version

# 4. 装官方 tauri-driver(cargo crate;官方 0.1.4 编不过,用 git HEAD)
cargo install --git https://github.com/tauri-apps/tauri --bin tauri-driver

# 5. clone 仓库 + 装 npm deps
git clone <your-repo> vibeterm
cd vibeterm
pnpm install --frozen-lockfile
cd e2e
pnpm add -D webdriverio @wdio/cli @wdio/local-runner @wdio/mocha-framework @wdio/spec-reporter @types/node ts-node
```

---

## 跑测试

```powershell
# 1. build release binary(WebView2 + 你的 webview = .exe)
pnpm tauri build --no-bundle  # 不需要 .msi,只要 .exe 给 wdio 用

# 2. 跑 wdio
cd e2e
pnpm wdio run wdio.windows.conf.ts
```

`wdio.windows.conf.ts` 会自动:
- 起 `tauri-driver`(它内部起 `msedgedriver` on port 4445)
- launch `target/release/vibeterm.exe`
- 对其 webview 派发真键盘事件
- 用 selector 查 DOM(看真 `[data-testid="palette-input"]`)
- 跑完后 kill 所有进程

---

## 跑哪些 spec

`specs/*.spec.ts` 写一次,Windows 上真跑。当前:
- `cmd-palette.spec.ts` —— 主 header 显示、Cmd+K(在 Win = Ctrl+K)打开命令面板、Esc 关闭、Ctrl+, 打开 Settings

---

## CI 集成

`.github/workflows/ci.yml` 加一个 `e2e-windows` job(`runs-on: windows-latest`):

```yaml
e2e-windows:
  runs-on: windows-latest
  steps:
    - uses: actions/checkout@v4
    - uses: pnpm/action-setup@v4
    - uses: actions/setup-node@v4
    - uses: dtolnay/rust-toolchain@stable
    - run: cargo install --git https://github.com/tauri-apps/tauri --bin tauri-driver
    - run: pnpm install --frozen-lockfile
    - run: pnpm tauri build --no-bundle
    - run: cd e2e && pnpm wdio run wdio.windows.conf.ts
```

`msedgedriver` 在 windows-latest runner 上预装(因为 Edge 预装)。

---

## 故障排查

| 报错 | 原因 | 修法 |
|---|---|---|
| `msedgedriver not found` | 没在 PATH | winget install Microsoft.Edge.WebDriver,或手动加 PATH |
| `session not created: This version of MSEdgeDriver only supports Edge version N` | Edge 自更新了,driver 没跟上 | 升级 msedgedriver 到与 Edge 同版本 |
| `connection refused on port 4444` | tauri-driver 没起来 | cargo install tauri-driver 检查;手动 `tauri-driver` 看错误 |
| 测试找不到 `[data-testid="..."]` | binary 不是最新 build | `pnpm tauri build --no-bundle` 重建 |

---

## 不在 Windows 上时

Mac/Linux 跑 `pnpm test:smoke`(Playwright + mock IPC,6 用例)就够日常开发。
真 WKWebView E2E 留给 Windows pipeline / 偶尔 macOS 手动加 CN_API_KEY 跑。
