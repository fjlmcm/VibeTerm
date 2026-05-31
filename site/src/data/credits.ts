// 致谢数据 —— 源:THIRD-PARTY-NOTICES.md + app about.inspiration.*
// 区块标题走 i18n;项目名/链接为事实,简述为英文中性描述。
// 无把握的链接宁可留空,只列名致谢,绝不写错 URL。

export interface CreditItem {
  name: string;
  url?: string;
  note?: string;
}

export interface CreditGroup {
  /** i18n key 后缀:credits.group.<id> */
  id: string;
  items: CreditItem[];
}

export const CREDIT_GROUPS: CreditGroup[] = [
  {
    id: 'framework',
    items: [
      { name: 'Tauri', url: 'https://tauri.app', note: 'Desktop runtime' },
      { name: 'SolidJS', url: 'https://solidjs.com', note: 'Reactive UI' },
      { name: 'xterm.js', url: 'https://xtermjs.org', note: 'Terminal frontend' },
    ],
  },
  {
    id: 'rust',
    items: [
      { name: 'portable-pty', url: 'https://github.com/wezterm/wezterm', note: 'PTY abstraction' },
      { name: 'tokio', url: 'https://tokio.rs' },
      { name: 'serde', url: 'https://serde.rs' },
      { name: 'notify', url: 'https://github.com/notify-rs/notify' },
      { name: 'thiserror', url: 'https://github.com/dtolnay/thiserror' },
      { name: 'tracing', url: 'https://github.com/tokio-rs/tracing' },
      { name: 'ureq', url: 'https://github.com/algesten/ureq', note: 'Manual update check only' },
    ],
  },
  {
    id: 'frontend',
    items: [
      { name: 'solid-dnd', url: 'https://github.com/thisbeyond/solid-dnd' },
      { name: 'html-to-image', url: 'https://github.com/bubkoo/html-to-image' },
      { name: 'Tauri plugins', note: 'clipboard / dialog / notification' },
    ],
  },
  {
    id: 'inspiration',
    items: [
      { name: 'ccusage', url: 'https://github.com/ryoppippi/ccusage', note: 'Usage aggregation, pricing, 5h blocks' },
      { name: 'WezTerm', url: 'https://github.com/wezterm/wezterm', note: 'PTY & terminal craft' },
      { name: 'Tabby', url: 'https://github.com/Eugeny/tabby', note: 'Terminal UX' },
      { name: 'LiteLLM', url: 'https://github.com/BerriAI/litellm', note: 'Model pricing reference' },
      { name: 'Prowl', note: 'Agent status inspiration' },
      { name: 'CodexBar', note: 'Status bar inspiration' },
      { name: 'ccstatusline', note: 'Status line inspiration' },
      { name: 'panzoom', url: 'https://github.com/anvaka/panzoom', note: 'Canvas pan/zoom' },
    ],
  },
  {
    id: 'assets',
    items: [
      { name: 'JetBrains Mono', url: 'https://github.com/JetBrains/JetBrainsMono', note: 'OFL-1.1' },
      { name: 'Color themes', note: 'Nord · Tokyo Night · Catppuccin · Solarized · Gruvbox · One Dark · GitHub' },
      { name: 'Pixabay', url: 'https://pixabay.com', note: 'Notification sounds' },
    ],
  },
];
