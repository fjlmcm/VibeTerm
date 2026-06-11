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

/** 回放分隔线:写在恢复的历史与新会话首个 prompt 之间(dim 样式)。 */
export const RESTORE_SEPARATOR = "\r\n\x1b[2m──── session restored ────\x1b[0m\r\n";

const RESTORE_MARKER_TEXT = "──── session restored ────";

// 仅用于"可见文本"比较:剥 SGR/CSI 与 OSC 序列(不改写回放数据本身)。
// eslint-disable-next-line no-control-regex
const ESC_SEQ = /\x1b(?:\[[0-9;:?]*[A-Za-z]|\][^\x07\x1b]*(?:\x07|\x1b\\))/g;
const visibleText = (line: string) => line.replace(ESC_SEQ, "");

// 剥掉历史快照里上次重启留下的回放痕迹,防止逐次重启无限累积:
//   1. 分隔线整行移除(SerializeAddon 往返后 SGR 前后缀不固定,按可见文本匹配);
//   2. 紧跟分隔线的一行若与上一保留行可见文本相同(= 上次"重启即关闭"会话只留下
//      一条裸 prompt),去重——只看分隔线后第一行,真实历史里的重复输出不受影响。
// 被丢弃行上的 SGR(\x1b[2m / \x1b[0m)成对消失,样式状态不残破。
function stripRestoreArtifacts(raw: string): string {
  const lines = raw.split("\r\n");
  const out: string[] = [];
  let afterMarker = false;
  for (const line of lines) {
    if (visibleText(line).includes(RESTORE_MARKER_TEXT)) {
      afterMarker = true;
      continue;
    }
    const isDupPrompt =
      afterMarker &&
      out.length > 0 &&
      visibleText(line) === visibleText(out[out.length - 1]);
    afterMarker = false;
    if (!isDupPrompt) out.push(line);
  }
  return out.join("\r\n");
}

/** 读取 key 对应的 scrollback,**不消费**。配合 commitScrollback 用:
 * spawn 成功且组件仍存活后才 commit——若在 peek 与 spawn 之间组件被卸载,
 * 内容保留给下次挂载,不会"消费了却没人看到"地永久丢失。
 * 返回前剥掉历史里的旧分隔线与重启残留(新分隔线由调用方在回放后追加一条)。 */
export function peekScrollback(key: string): string | null {
  if (!saved) return null;
  const raw = saved[key];
  return raw ? stripRestoreArtifacts(raw) : null;
}

/** 消费 key 对应的 scrollback(防同一会话内 re-mount 重复回放)。 */
export function commitScrollback(key: string): void {
  if (saved) delete saved[key];
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
