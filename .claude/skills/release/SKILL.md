---
name: release
description: VibeTerm 提交 / 版本 / 发版流程 —— 提交前验证(尤其 cargo fmt,CI lint 必查)、何时 bump 版本、tag 触发签名公证自动发布。每次准备提交、推送或发版前调用,避免漏项(本仓多次栽在漏跑 cargo fmt 上)。
---

# VibeTerm 发版流程

提交 / 推送 / 发版前按本清单走。**根目录是 monorepo,Rust 在 `src-tauri/`,web 在根用 pnpm。**

## 1. 提交前验证闸门(顺序固定,全绿才提交)

> ⚠️ **头号坑:`cargo fmt`**。CI 的 lint job 在 **macOS** 跑 `cargo fmt --check`,漏跑 fmt → CI 直接红(v1.0.0、v1.0.1 都栽过)。`clippy` / `test` / `typecheck` 全过也救不了——它们不查格式。

```bash
cd src-tauri
export VIBETERM_CONFIG_DIR=/tmp/vt-$$        # 隔离:别抢用户真实 tasks.json

cargo fmt --all                              # ① 永远先跑(漏它 = CI lint 红)
cargo fmt --all --check                      #    确认干净(无输出)
cargo clippy --workspace --all-targets -- -D warnings   # ② -D warnings,含 doc_lazy_continuation 等
cargo test -p vibeterm-config -p vibeterm-core -p vibeterm-ipc \
           -p vibeterm-pty -p vibeterm-status -p vibeterm-tasks \
           -p vibeterm-agent-watch           # ③ 6 子 crate + agent-watch(排除链 tauri 的主 crate)
```

```bash
cd ..                                        # 回根
pnpm typecheck                               # ④ 仅当 web/ 改了
python3 scripts/gen-readme.py                # ⑤ 仅当改了官网 i18n 文案 / README 结构
```

- 本地复现 CI clippy 失败,先 `rustup update stable` 对齐版本(旧 clippy 漏报)。

## 2. 版本号(何时 bump)

权威源 `src-tauri/tauri.conf.json`,`scripts/bump-version.py` 一处改、lockstep 同步 6 个 package.json + Cargo workspace。

```bash
python3 scripts/bump-version.py            # patch  x.y.z → x.y.(z+1)
python3 scripts/bump-version.py minor      # / major / 0.5.2(设具体版本)
(cd src-tauri && cargo check)              # bump 后更新 Cargo.lock
```

- **要发布的 app 代码改动** → bump。
- **纯文档 / README / CI 配置 / 脚本改动** → **不 bump**(版本随下次真发版带走)。
- HEAD 已是目标版本(之前 bump 过但没发)→ 直接发,别再 bump。
- 已推送的提交别 amend 重写,累积新提交即可。

## 3. 提交 + 推送

- 约定式提交:`feat / fix / docs / perf / refactor / test / chore / ci`。归属已全局禁用(无 Co-Authored-By)。
- 直接提交 `main`(本仓既有工作流,单人项目)。

```bash
git add -A && git commit -m "<type>: <简述>"
git push origin main
```

## 4. 发版(tag → 自动发布)

```bash
git tag vX.Y.Z && git push origin vX.Y.Z    # 版本须与 Cargo/tauri 一致
```

- 触发 `.github/workflows/release.yml`:macOS universal(**签名 + 公证**)+ Windows x64 → 产物 dmg / app.tar.gz / exe / msi。
- **2026-06-01 起 `releaseDraft: false` → 自动 publish 并设 Latest,不再生成草稿、无需手动 publish。**
- 多平台 matrix:先完成的 job 先发布,后完成平台产物随后追加(发布后短时可能暂缺某平台二进制)。

## 5. 推送后确认(必做)

```bash
gh run list --limit 4                                   # CI + Release 状态
gh run watch <run-id> --exit-status                     # 阻塞等结果(0=绿)
```

CI 失败先看是不是 `cargo fmt`(回第 1 步)。发版失败常见:`APPLE_SIGNING_IDENTITY` 等 secret **尾随空格/换行**(日志 identity 末尾有空格是线索;`gh secret set` 重设去空白,证书名是公开值非密钥)。

## 速记
1. **cargo fmt --all**(别忘!)→ clippy -D warnings → test 6 crate → typecheck →(改文案则 gen-readme)
2. 该 bump 才 bump(纯文档不 bump)
3. commit + push main
4. tag vX.Y.Z + push → 自动签名公证发布
5. `gh run watch` 确认绿
