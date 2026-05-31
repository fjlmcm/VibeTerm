// 把文件路径以 shell 安全的方式拼成一行,塞回终端。
//
// 规则:
//   - 无空格/特殊字符 → 原样
//   - 含空格或 shell 元字符 → 单引号包裹
//   - 路径自身含单引号 → 用 POSIX 标准的 `'\''` 转义
//   - 多个路径用空格分隔

const SAFE_CHARS = /^[A-Za-z0-9_\-./:@%+,=]+$/;

/** 单条路径的 shell 安全形式 */
export function shellQuoteOne(path: string): string {
  if (SAFE_CHARS.test(path)) return path;
  // POSIX 单引号转义:foo'bar → 'foo'\''bar'
  return "'" + path.replace(/'/g, "'\\''") + "'";
}

/** 多条路径拼成一行(空格分隔,各自按需引号) */
export function shellQuotePaths(paths: readonly string[]): string {
  return paths.map(shellQuoteOne).join(" ");
}
