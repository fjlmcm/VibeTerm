// 核心特性结构 —— 文案走 i18n(features.* key),此处只定义分组/图标/顺序。
// 与产品 about.*/features.* 对齐(src 来源:web/packages/ui-core/src/i18n)。

export type FeatureGroup = 'agent' | 'terminal' | 'productivity';

export interface Feature {
  /** i18n key 后缀:features.<group>.<id> 的 label / desc */
  id: string;
  group: FeatureGroup;
  /** 图标语义名(组件层映射为内联 SVG) */
  icon: string;
}

export const FEATURE_GROUPS: FeatureGroup[] = ['agent', 'terminal', 'productivity'];

export const FEATURES: Feature[] = [
  { id: 'sniff', group: 'agent', icon: 'radar' },
  { id: 'urgency', group: 'agent', icon: 'list-ordered' },
  { id: 'monitor', group: 'agent', icon: 'activity' },
  { id: 'stats', group: 'agent', icon: 'bar-chart' },
  { id: 'split', group: 'terminal', icon: 'columns' },
  { id: 'canvas', group: 'terminal', icon: 'layout-grid' },
  { id: 'floating', group: 'terminal', icon: 'picture-in-picture' },
  { id: 'render', group: 'terminal', icon: 'cpu' },
  { id: 'palette', group: 'productivity', icon: 'command' },
  { id: 'prompts', group: 'productivity', icon: 'sparkles' },
  { id: 'statusbar', group: 'productivity', icon: 'panel-bottom' },
  { id: 'notify', group: 'productivity', icon: 'bell' },
  { id: 'theme', group: 'productivity', icon: 'palette' },
];

export function featuresByGroup(group: FeatureGroup): Feature[] {
  return FEATURES.filter((f) => f.group === group);
}
