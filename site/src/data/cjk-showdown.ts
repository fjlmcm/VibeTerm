// CJK 实锤对比数据 —— 源:docs/CJK_SHOWDOWN.md
// 立场:事实判断依据,不抹黑;每条都指向公开 GitHub issue。
// 描述保留英文(issue 原生语言),区块标题/结论/列头走 i18n。

export type IssueStatus = 'open' | 'closed-unfixed' | 'partial';

export interface CjkIssue {
  tool: string;
  /** 简短英文描述(贴近 issue 原文) */
  summary: string;
  /** issue 编号(显示用) */
  ref: string;
  url: string;
  /** 👍 reactions(若公开可见且有代表性) */
  thumbs?: number;
  status: IssueStatus;
}

export const CJK_ISSUES: CjkIssue[] = [
  {
    tool: 'Claude Code',
    summary: 'Japanese IME lag + duplicate candidate window',
    ref: '#1547',
    url: 'https://github.com/anthropics/claude-code/issues/1547',
    thumbs: 241,
    status: 'open',
  },
  {
    tool: 'Claude Code',
    summary: 'Enter fires mid-IME-composition',
    ref: '#8405',
    url: 'https://github.com/anthropics/claude-code/issues/8405',
    thumbs: 95,
    status: 'closed-unfixed',
  },
  {
    tool: 'Claude Code',
    summary: 'Streaming UTF-8 boundary corrupts CJK',
    ref: '#45508',
    url: 'https://github.com/anthropics/claude-code/issues/45508',
    status: 'open',
  },
  {
    tool: 'Claude Code',
    summary: 'CJK table misalignment',
    ref: '#13438',
    url: 'https://github.com/anthropics/claude-code/issues/13438',
    status: 'open',
  },
  {
    tool: 'Claude Code',
    summary: 'Chinese line-wrap truncation regression',
    ref: '#14812',
    url: 'https://github.com/anthropics/claude-code/issues/14812',
    status: 'open',
  },
  {
    tool: 'Claude Code',
    summary: 'Korean + box-drawing truncation',
    ref: '#14597',
    url: 'https://github.com/anthropics/claude-code/issues/14597',
    status: 'open',
  },
  {
    tool: 'Warp',
    summary: 'No Simplified-Chinese UI',
    ref: '#9357',
    url: 'https://github.com/warpdotdev/warp/issues/9357',
    status: 'open',
  },
  {
    tool: 'Warp',
    summary: 'Garbled Chinese filenames in sidebar',
    ref: '#7436',
    url: 'https://github.com/warpdotdev/warp/issues/7436',
    status: 'open',
  },
  {
    tool: 'cmux',
    summary: "Forced font injection — CJK users can't override",
    ref: '#4519',
    url: 'https://github.com/manaflow-ai/cmux/issues/4519',
    status: 'open',
  },
  {
    tool: 'cmux',
    summary: 'Korean font force-injected',
    ref: '#1693',
    url: 'https://github.com/manaflow-ai/cmux/issues/1693',
    status: 'open',
  },
  {
    tool: 'Ghostty',
    summary: 'macOS Chinese punctuation positioning bug',
    ref: '#12173',
    url: 'https://github.com/ghostty-org/ghostty/issues/12173',
    status: 'partial',
  },
  {
    tool: 'xterm.js',
    summary: 'wcwidth historical bug chain',
    ref: '#1059',
    url: 'https://github.com/xtermjs/xterm.js/issues/1059',
    status: 'partial',
  },
  {
    tool: 'Microsoft Terminal',
    summary: 'CJK ambiguous width',
    ref: '#370',
    url: 'https://github.com/microsoft/terminal/issues/370',
    status: 'open',
  },
  {
    tool: 'Microsoft Terminal',
    summary: 'Background activity notification — 5-yr spec, unimplemented',
    ref: '#7955',
    url: 'https://github.com/microsoft/terminal/issues/7955',
    status: 'open',
  },
];

/** VibeTerm 在每个维度的对照立场(i18n key 后缀:cjk.point.<id>) */
export const CJK_POINTS: string[] = ['ime', 'width', 'wrap', 'copy', 'render', 'notify'];
