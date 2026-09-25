#!/usr/bin/env python3
"""刷新内嵌模型上下文窗口快照。

从 LiteLLM 社区模型表(model_prices_and_context_window.json)抽取 anthropic 原生
claude 条目的 max_input_tokens,写入 vibeterm-agent-watch 的内嵌快照:

    src-tauri/crates/vibeterm-agent-watch/src/claude/litellm_snapshot.json

该快照编译进二进制,供应用离线查 model id → 上下文窗口。
**每次发布新版本前运行一次本脚本**(发版流程见 .claude/skills/release),
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
KEEP_FIELDS = ("litellm_provider", "max_input_tokens")

# 裸 model id 在 Claude Code 里默认 200k、1M 需 [1m] 后缀的模型(见 main 里的说明)。
CONTEXT_PINS = {
    "claude-sonnet-4-5": 200_000,
    "claude-opus-4-5": 200_000,
}

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
        if v.get("max_input_tokens") is None:
            continue
        entries[key] = {f: v[f] for f in KEEP_FIELDS if v.get(f) is not None}
    if len(entries) < 10:
        sys.exit(f"abort: only {len(entries)} entries extracted — source format changed?")

    old = {}
    if SNAPSHOT_PATH.exists():
        old = json.loads(SNAPSHOT_PATH.read_text()).get("entries", {})

    # 上下文窗口取"Claude Code 默认会话"的值而非 API 上限:1M 会话在 transcript 里带 [1m]
    # 后缀(context_window_for 单独识别),裸 id 仍是 200k;LiteLLM 的 max_input_tokens 反映
    # 开 beta header 后的上限,照抄会让 ctx% 低 5 倍。仅对已知"裸 id 默认 200k"的模型钉住。
    pinned = []
    for k, v in entries.items():
        for prefix, ctx in CONTEXT_PINS.items():
            if (k == prefix or k.startswith(prefix + "-")) and v.get("max_input_tokens") != ctx:
                v["max_input_tokens"] = ctx
                pinned.append(k)

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
    for tag, names in (
        ("added", added),
        ("removed", removed),
        ("context pinned", sorted(pinned)),
        ("changed", changed),
    ):
        if names:
            print(f"  {tag}: {', '.join(names)}")
    if not (added or removed or changed):
        print("  no changes")


if __name__ == "__main__":
    main()
