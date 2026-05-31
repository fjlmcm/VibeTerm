#!/usr/bin/env python3
"""统一管理 VibeTerm 版本号 —— monorepo 多处保持 lockstep 一致。

权威版本源: src-tauri/tauri.conf.json
一处 bump,同步到所有 package.json + Cargo workspace。

用法:
  python scripts/bump-version.py            # patch: 0.3.0 -> 0.3.1
  python scripts/bump-version.py minor      #        0.3.0 -> 0.4.0
  python scripts/bump-version.py major      #        0.3.0 -> 1.0.0
  python scripts/bump-version.py 0.5.2      # 设为具体版本

bump 后记得跑 `cargo check`(更新 Cargo.lock)+ `pnpm install`(更新 pnpm-lock)再提交。
打 tag 发版: git tag vX.Y.Z && git push origin vX.Y.Z
"""
import json
import re
import sys
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent

# 需同步版本号的 JSON 文件(顶层 "version")
JSON_FILES = [
    "package.json",
    "src-tauri/tauri.conf.json",
    "site/package.json",
    "web/packages/main/package.json",
    "web/packages/ipc-types/package.json",
    "web/packages/ui-core/package.json",
]
# Cargo workspace 版本(子 crate 走 version.workspace = true 自动继承)
CARGO_TOML = "src-tauri/Cargo.toml"

SEMVER = re.compile(r"^\d+\.\d+\.\d+$")


def current_version() -> str:
    data = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    return data["version"]


def next_version(cur: str, kind: str) -> str:
    if SEMVER.match(kind):
        return kind
    major, minor, patch = (int(x) for x in cur.split("."))
    if kind == "major":
        return f"{major + 1}.0.0"
    if kind == "minor":
        return f"{major}.{minor + 1}.0"
    if kind == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise SystemExit(f"未知参数: {kind}(用 patch/minor/major 或具体版本如 0.5.2)")


def set_json_version(rel: str, new: str) -> None:
    p = ROOT / rel
    s = p.read_text(encoding="utf-8")
    # 只替换顶层第一个 "version": "..."(package.json 依赖不用 "version" 键,安全)
    s2, n = re.subn(r'("version":\s*)"[^"]*"', rf'\1"{new}"', s, count=1)
    if n == 0:
        raise SystemExit(f"✗ 没在 {rel} 找到 version 字段")
    p.write_text(s2, encoding="utf-8")


def set_cargo_version(new: str) -> None:
    p = ROOT / CARGO_TOML
    s = p.read_text(encoding="utf-8")
    # 只改行首 `version = "x.y.z"`(= [workspace.package] 的版本),
    # 不碰依赖的 `tauri = { version = "2" }` / 多行依赖里的 `version = "6"`
    s2, n = re.subn(r'(?m)^version = "[^"]*"', f'version = "{new}"', s, count=1)
    if n == 0:
        raise SystemExit(f"✗ 没在 {CARGO_TOML} 找到 workspace version")
    p.write_text(s2, encoding="utf-8")


def main() -> None:
    kind = sys.argv[1] if len(sys.argv) > 1 else "patch"
    cur = current_version()
    new = next_version(cur, kind)
    for f in JSON_FILES:
        set_json_version(f, new)
    set_cargo_version(new)
    # 官网 site.ts 的 VERSION 常量(TS,单引号)
    site_ts = ROOT / "site/src/lib/site.ts"
    sts = site_ts.read_text(encoding="utf-8")
    sts2, n = re.subn(r"(export const VERSION = ')[^']*(')", rf"\g<1>{new}\g<2>", sts)
    if n:
        site_ts.write_text(sts2, encoding="utf-8")
    print(f"version: {cur} -> {new}")
    print("已同步:", ", ".join(JSON_FILES + [CARGO_TOML, "site/src/lib/site.ts"]))
    print("下一步: cargo check(更新 Cargo.lock) + pnpm install(更新 pnpm-lock),然后提交。")


if __name__ == "__main__":
    main()
