#!/usr/bin/env python3
"""刷新内嵌模型数据快照(价格 + 上下文窗口)。

从 LiteLLM 社区价格表(model_prices_and_context_window.json,ccusage 同源)抽取
anthropic 原生 claude 条目,写入 vibeterm-agent-watch 的内嵌快照:

    src-tauri/crates/vibeterm-agent-watch/src/claude/litellm_snapshot.json

该快照编译进二进制,作为离线兜底;设置·更新页"更新模型价格"在运行时拉同一数据源
做覆盖。**每次发布新版本前运行一次本脚本**(发版流程见 .claude/skills/release),
有 diff 随版本提交,保证内置数据不过时。

用法:
    python3 scripts/update-model-data.py            # 联网拉取最新
    python3 scripts/update-model-data.py --from F   # 从本地 LiteLLM JSON 文件读(测试/离线)
"""

import datetime
import json
import sys
import urllib.request
from pathlib import Path

LITELLM_URL = (
    "https://raw.githubusercontent.com/BerriAI/litellm/main/"
    "model_prices_and_context_window.json"
)
SNAPSHOT_PATH = (
    Path(__file__).resolve().parent.parent
    / "src-tauri/crates/vibeterm-agent-watch/src/claude/litellm_snapshot.json"
)

# 只保留 Rust 侧转换会用到的字段, 控制内嵌体积.
KEEP_FIELDS = (
    "litellm_provider",
    "max_input_tokens",
    "input_cost_per_token",
    "output_cost_per_token",
    "cache_creation_input_token_cost",
    "cache_read_input_token_cost",
    "input_cost_per_token_above_200k_tokens",
    "output_cost_per_token_above_200k_tokens",
    "cache_creation_input_token_cost_above_200k_tokens",
    "cache_read_input_token_cost_above_200k_tokens",
)

def fetch_source() -> dict:
    if len(sys.argv) >= 3 and sys.argv[1] == "--from":
        return json.loads(Path(sys.argv[2]).read_text())
    req = urllib.request.Request(LITELLM_URL, headers={"User-Agent": "VibeTerm-scripts"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read())


def main() -> None:
    src = fetch_source()
    entries = {}
    for key, v in src.items():
        if not isinstance(v, dict):
            continue
        if v.get("litellm_provider") != "anthropic":
            continue
        if not key.lower().startswith("claude"):
            continue
        if v.get("input_cost_per_token") is None or v.get("output_cost_per_token") is None:
            continue
        entries[key] = {f: v[f] for f in KEEP_FIELDS if v.get(f) is not None}
    if len(entries) < 10:
        sys.exit(f"abort: only {len(entries)} entries extracted — source format changed?")

    old = {}
    if SNAPSHOT_PATH.exists():
        old = json.loads(SNAPSHOT_PATH.read_text()).get("entries", {})
    snapshot = {
        "snapshot_date": datetime.date.today().isoformat(),
        "source": "LiteLLM (BerriAI/litellm)",
        "entries": dict(sorted(entries.items())),
    }
    SNAPSHOT_PATH.write_text(json.dumps(snapshot, indent=2, sort_keys=False) + "\n")

    added = sorted(set(entries) - set(old))
    removed = sorted(set(old) - set(entries))
    changed = sorted(k for k in set(entries) & set(old) if entries[k] != old[k])
    print(f"wrote {SNAPSHOT_PATH.relative_to(Path.cwd())} ({len(entries)} models)")
    for tag, names in (("added", added), ("removed", removed), ("changed", changed)):
        if names:
            print(f"  {tag}: {', '.join(names)}")
    if not (added or removed or changed):
        print("  no changes")


if __name__ == "__main__":
    main()
