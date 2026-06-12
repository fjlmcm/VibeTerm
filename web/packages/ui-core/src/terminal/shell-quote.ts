// 把文件路径以 shell 安全的方式拼成一行,塞回终端。
//
// POSIX(macOS / Linux)规则:
//   - 无空格/特殊字符 → 原样
//   - 含空格或 shell 元字符 → 单引号包裹
//   - 路径自身含单引号 → 用 POSIX 标准的 `'\''` 转义
// Windows(pwsh / powershell / cmd)规则:
//   - 反斜杠是路径分隔符而非转义符,计入安全字符
//   - 需要引号时用双引号 —— pwsh / powershell / cmd 三者都认;
//     POSIX 单引号 cmd 完全不认、`'\''` 转义 PowerShell 也不认。
//     Windows 文件名不可能含 `"`(NTFS 非法字符),无需内部转义。
//   - 多个路径用空格分隔

import { isWindowsPlatform } from "../keybindings";

const SAFE_CHARS_POSIX = /^[A-Za-z0-9_\-./:@%+,=]+$/;
const SAFE_CHARS_WIN = /^[A-Za-z0-9_\-.\\/:@+,=~]+$/;

/** 单条路径的 shell 安全形式 */
export function shellQuoteOne(path: string): string {
  if (isWindowsPlatform()) {
    if (SAFE_CHARS_WIN.test(path)) return path;
    return `"${path}"`;
  }
  if (SAFE_CHARS_POSIX.test(path)) return path;
  // POSIX 单引号转义:foo'bar → 'foo'\''bar'
  return "'" + path.replace(/'/g, "'\\''") + "'";
}

/** 多条路径拼成一行(空格分隔,各自按需引号) */
export function shellQuotePaths(paths: readonly string[]): string {
  return paths.map(shellQuoteOne).join(" ");
}
