// 用户可见文本的 grapheme 级工具(CJK 一等公民:按 code unit 截断会撕裂代理对 / ZWJ 序列)。

function graphemes(s: string): string[] {
  if (typeof (Intl as { Segmenter?: unknown }).Segmenter === "function") {
    try {
      const seg = new Intl.Segmenter(undefined, { granularity: "grapheme" });
      return Array.from(seg.segment(s), (x) => x.segment);
    } catch {
      /* 退化为 code point 切分 */
    }
  }
  return Array.from(s);
}

/** 按 grapheme 截到最多 n 个;fromEnd 时保留尾部。不足 n 个原样返回。 */
export function truncateGraphemes(s: string, n: number, opts?: { fromEnd?: boolean }): string {
  const g = graphemes(s);
  if (g.length <= n) return s;
  return (opts?.fromEnd ? g.slice(g.length - n) : g.slice(0, n)).join("");
}
