// G5:会话 scrollback best-effort 恢复。
//
// 🟢 零侵入:序列化的终端缓冲按 "taskId:slotId" 键存 VibeTerm 自己的 scrollback.json。
// 重启后每个 task/slot 的终端重建时回放旧缓冲(纯展示;旧 shell 进程已不在)。
// 布局 / cwd 本就由 tasks.json 持久化,这里只补"看得见的历史"。
//
// 设计:
//   - loadSavedScrollback():启动读一次,缓存在内存。
//   - takeScrollback(key):取出并**消费一次**(防同一会话内 re-mount 重复回放)。
//   - registerScrollbackSnapshot(key, fn):每个活终端注册一个序列化函数。
//   - startScrollbackAutosave():定时 + pagehide/beforeunload 收集全部 → 落盘。
import * as ipc from "./ipc";

let saved: Record<string, string> | null = null;
const snapshotFns = new Map<string, () => string>();

/** 启动加载一次(失败降级为空,不报错)。 */
export async function loadSavedScrollback(): Promise<void> {
  try {
    saved = await ipc.loadScrollback();
  } catch {
    saved = {};
  }
}

/** 取出并消费 key 对应的 scrollback(只回放一次)。 */
export function takeScrollback(key: string): string | null {
  if (!saved) return null;
  const v = saved[key];
  if (v === undefined) return null;
  delete saved[key];
  return v;
}

/** 注册一个终端的序列化函数(返回注销函数)。 */
export function registerScrollbackSnapshot(key: string, fn: () => string): () => void {
  snapshotFns.set(key, fn);
  return () => {
    if (snapshotFns.get(key) === fn) snapshotFns.delete(key);
  };
}

// 剥离 OSC 10/11/12(主题前景/背景/光标色),防换主题后回放串色(对齐红线:配色按数据排查)。
function stripThemeOsc(s: string): string {
  // eslint-disable-next-line no-control-regex
  return s.replace(/\x1b\](1[012]);[^\x07\x1b]*(?:\x07|\x1b\\)/g, "");
}

function collectEntries(): { key: string; data: string }[] {
  const out: { key: string; data: string }[] = [];
  for (const [key, fn] of snapshotFns) {
    try {
      const data = stripThemeOsc(fn());
      if (data.trim().length > 0) out.push({ key, data });
    } catch {
      /* 单个终端序列化失败不影响其余 */
    }
  }
  return out;
}

let timer: ReturnType<typeof setInterval> | null = null;

/** 启动定期 + 退出前自动保存 scrollback(幂等,重复调用无副作用)。 */
export function startScrollbackAutosave(intervalMs = 20000): void {
  if (timer) return;
  const flush = () => {
    const e = collectEntries();
    if (e.length) ipc.saveScrollback(e).catch(() => {});
  };
  timer = setInterval(flush, intervalMs);
  window.addEventListener("pagehide", flush);
  window.addEventListener("beforeunload", flush);
}
