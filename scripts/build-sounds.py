#!/usr/bin/env python3
"""压缩 tmp/*.mp3 → src-tauri/resources/sounds/<id>.mp3 + sounds.json 清单.

策略 (通知音场景损失不可感):
  - mono       (通知不需要立体声)
  - 22050 Hz   (人耳通知音频谱基本在 4 kHz 内, 22050 已绰绰有余)
  - 64 kbps    (在通知音长度 < 2s 下听感无损)
  - 自动去静音 (start_silence > 50ms 的前导静音剪掉, 提升响应感)

依赖:ffmpeg (brew install ffmpeg).

跑法:
    python3 scripts/build-sounds.py

输出:
    src-tauri/resources/sounds/<id>.mp3   (24 个文件)
    src-tauri/resources/sounds/sounds.json (清单 — 前端下拉用)
"""

import json
import shutil
import subprocess
from pathlib import Path


# (id, source_filename_substring, display_name, category)
# id 要稳定 + 简短 (前端展示用 display_name)
SOUND_MAP = [
    # 短促 ding / 单音 — 适合 WaitingInput
    ("ding1", "dragon-studio-new-notification-1-", "Ding 1", "notification"),
    ("ding3", "dragon-studio-new-notification-3-", "Ding 3", "notification"),
    ("bell", "dragon-studio-notification-bell-sound", "Bell", "notification"),
    ("ping", "dragon-studio-notification-ping-", "Ping", "notification"),
    ("effect", "dragon-studio-notification-sound-effect-", "Effect", "notification"),
    ("tone09", "universfield-new-notification-09-", "Tone 9", "notification"),
    ("tone17", "universfield-new-notification-017-", "Tone 17", "notification"),
    ("tone20", "universfield-new-notification-020-", "Tone 20", "notification"),
    ("tone21", "universfield-new-notification-021-", "Tone 21", "notification"),
    ("tone22", "universfield-new-notification-022-", "Tone 22", "notification"),
    ("tone24", "universfield-new-notification-024-", "Tone 24", "notification"),
    ("tone26", "universfield-new-notification-026-", "Tone 26", "notification"),
    # 旋律 / 音色 — 适合 Done
    ("passage", "soundreality-notification-passage-", "Passage", "tone"),
    ("piano", "soundreality-notification-piano-", "Piano", "tone"),
    ("puretone", "soundreality-notification-tone-", "Pure Tone", "tone"),
    ("type11", "ribhavagrawal-notification-sound-type-11", "Type 11", "tone"),
    ("type12", "ribhavagrawal-notification-sound-type-12", "Type 12", "tone"),
    # 语音 — 趣味
    ("voice-yes", "floraphonic-woman-excited-cheers-and-phrases-says-yes", "Voice: Yes", "voice"),
    ("voice-woo", "floraphonic-woman-excited-cheers-and-phrases-says-woo", "Voice: Woo", "voice"),
    ("voice-ya", "floraphonic-woman-excited-cheers-and-phrases-ya", "Voice: Ya", "voice"),
    # UI / 其它
    ("success", "soundshelfstudio-ui-success-chime", "Success Chime", "ui"),
    ("mountain-king", "grimgravy-the-hall-of-the-mountain-king", "Mountain King", "other"),
    ("ringtone1", "universfield-ringtone-021-", "Ringtone 1", "ringtone"),
    ("ringtone2", "lucadialessandro-ringtone-", "Ringtone 2", "ringtone"),
]


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    src_dir = repo_root / "tmp"
    out_dir = repo_root / "src-tauri" / "resources" / "sounds"
    manifest_path = out_dir / "sounds.json"

    if not src_dir.is_dir():
        print(f"FATAL: source dir {src_dir} not found")
        return 1
    if shutil.which("ffmpeg") is None:
        print("FATAL: ffmpeg not on PATH (brew install ffmpeg)")
        return 1

    out_dir.mkdir(parents=True, exist_ok=True)

    manifest = {"sounds": []}
    total_in = 0
    total_out = 0
    failed = []

    for sound_id, src_prefix, display, category in SOUND_MAP:
        matches = sorted(src_dir.glob(f"{src_prefix}*.mp3"))
        if not matches:
            failed.append((sound_id, "source not found"))
            print(f"  ✗ {sound_id:<14} — no match for prefix {src_prefix!r}")
            continue
        src = matches[0]
        out = out_dir / f"{sound_id}.mp3"

        try:
            subprocess.run(
                [
                    "ffmpeg",
                    "-y",
                    "-loglevel", "error",
                    "-i", str(src),
                    "-ac", "1",
                    "-ar", "22050",
                    "-b:a", "64k",
                    "-af",
                    "silenceremove=start_periods=1:start_silence=0.05:start_threshold=-50dB",
                    str(out),
                ],
                check=True,
            )
        except subprocess.CalledProcessError as e:
            failed.append((sound_id, str(e)))
            print(f"  ✗ {sound_id:<14} — ffmpeg failed")
            continue

        in_kb = src.stat().st_size / 1024
        out_kb = out.stat().st_size / 1024
        total_in += in_kb
        total_out += out_kb
        ratio = 100 * out_kb / in_kb if in_kb else 0
        print(f"  ✓ {sound_id:<14} {in_kb:>6.1f} KB → {out_kb:>5.1f} KB  ({ratio:5.1f}%)  {display}")

        manifest["sounds"].append(
            {
                "id": sound_id,
                "name": display,
                "category": category,
                "file": f"{sound_id}.mp3",
            }
        )

    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")

    print()
    print(f"  total {total_in:>7.1f} KB → {total_out:>6.1f} KB  ({100 * total_out / total_in:.1f}%)")
    print(f"  → {out_dir}")
    print(f"  → {manifest_path.relative_to(repo_root)}")
    if failed:
        print()
        print(f"  {len(failed)} failed:")
        for sid, why in failed:
            print(f"    - {sid}: {why}")
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
