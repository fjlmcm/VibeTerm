// 客户端主题切换 —— 设 data-theme / data-appearance + 持久化 + 广播事件。
// 防 FOUC 的初始化是 Base.astro 里的 is:inline 脚本(硬编码同一 key)。
import { DEFAULT_THEME_ID, themeById, THEMES } from '../data/themes';

export const THEME_STORAGE_KEY = 'vibeterm-theme';

export function applyTheme(id: string): void {
  const theme = themeById(id);
  const root = document.documentElement;
  root.setAttribute('data-theme', theme.id);
  root.setAttribute('data-appearance', theme.appearance);
  try {
    localStorage.setItem(THEME_STORAGE_KEY, theme.id);
  } catch {
    /* localStorage 不可用(隐私模式)时静默,主题仍在本次会话生效 */
  }
  window.dispatchEvent(
    new CustomEvent('vt:themechange', {
      detail: { id: theme.id, appearance: theme.appearance },
    }),
  );
}

export function currentThemeId(): string {
  return document.documentElement.getAttribute('data-theme') ?? DEFAULT_THEME_ID;
}

export function cycleTheme(dir: 1 | -1 = 1): void {
  const ids = THEMES.map((t) => t.id);
  const idx = ids.indexOf(currentThemeId());
  const next = ids[(idx + dir + ids.length) % ids.length];
  applyTheme(next);
}
