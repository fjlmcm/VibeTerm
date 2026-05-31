// 通知声音播放
//
// 后端把音频字节 base64 回传 → 这里转 Blob URL → <audio> 播放.
// 预览按钮 (设置面板) 和 runtime 自定义文件通知 (notification_play_sound 事件) 都走此路径.
//
// 设计要点:
//   - 同一时刻只放一个声音 (避免重复点 preview 按钮叠声)
//   - 播完自动 revokeObjectURL, 防内存泄漏
//   - 缓存 base64 → Blob URL: runtime 通知反复触发同一声音不重复解码

import { previewNotifySound } from "./ipc";

let currentAudio: HTMLAudioElement | null = null;

// LRU-ish 缓存: 最多 4 个最近用过的 sound, 命中直接复用 Blob URL.
// audio 文件 < 1MB, 4 个占内存 < 4MB, 可忽略.
const cache = new Map<string, { url: string; mime: string }>();
const CACHE_MAX = 4;

function base64ToBlob(b64: string, mime: string): Blob {
  const binary = atob(b64);
  const len = binary.length;
  const buf = new Uint8Array(len);
  for (let i = 0; i < len; i++) buf[i] = binary.charCodeAt(i);
  return new Blob([buf], { type: mime });
}

function evictOldest() {
  if (cache.size <= CACHE_MAX) return;
  const first = cache.keys().next();
  if (first.done) return;
  const k = first.value;
  const e = cache.get(k);
  if (e) URL.revokeObjectURL(e.url);
  cache.delete(k);
}

function stopCurrent() {
  if (currentAudio) {
    try {
      currentAudio.pause();
      currentAudio.src = "";
    } catch {
      /* ignore */
    }
    currentAudio = null;
  }
  // 当前 url 来自 cache, 不 revoke (cache 还要复用).
}

async function urlFor(sound: string): Promise<string | null> {
  const cached = cache.get(sound);
  if (cached) {
    // LRU 触摸 — Map 保留插入序, 先删再插 = 移到末尾
    cache.delete(sound);
    cache.set(sound, cached);
    return cached.url;
  }
  let data;
  try {
    data = await previewNotifySound(sound);
  } catch (e) {
    console.warn("[notify-audio] preview failed", sound, e);
    return null;
  }
  let url: string;
  try {
    // atob 对非法 base64 会抛 DOMException — 捕获后返回 null 而非 reject
    const blob = base64ToBlob(data.base64, data.mime);
    url = URL.createObjectURL(blob);
  } catch (e) {
    console.warn("[notify-audio] decode failed", sound, e);
    return null;
  }
  cache.set(sound, { url, mime: data.mime });
  evictOldest();
  return url;
}

/**
 * 播放指定声音; 中断当前正在播的(如果有).
 * `sound` 是 NotifyPrefs.sound 字段格式: 系统名 (Glass) / 绝对路径 / "~/...".
 *
 * 返回 boolean: true 表示成功开始播放, false 表示解析失败 / 没匹配到文件.
 * 失败时调用方应静默(后端已有自己的 fallback).
 */
export async function playNotifySound(sound: string): Promise<boolean> {
  stopCurrent();
  const url = await urlFor(sound);
  if (!url) return false;
  const audio = new Audio(url);
  currentAudio = audio;
  audio.addEventListener("ended", () => {
    if (currentAudio === audio) {
      currentAudio = null;
    }
  });
  try {
    await audio.play();
    return true;
  } catch (e) {
    console.warn("[notify-audio] play failed", e);
    stopCurrent();
    return false;
  }
}

/** 停止当前预览 (设置面板"停止"按钮) */
export function stopNotifySound() {
  stopCurrent();
}

/** App unmount 时清理所有 Blob URL */
export function disposeNotifyAudio() {
  stopCurrent();
  for (const { url } of cache.values()) URL.revokeObjectURL(url);
  cache.clear();
}
