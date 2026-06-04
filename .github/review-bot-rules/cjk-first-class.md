# 规则:CJK 一等公民(HIGH)

改终端文本 / 复制 / 输入相关代码,必须顾及 CJK 正确性。

## 审查要点(改到终端文本 / 剪贴板 / 键盘输入路径时)

- **字素**:用 Unicode 15 grapheme / `Intl.Segmenter` 处理,不得按 UTF-16 code unit 或 `.length`
  切分(会撕裂代理对 / ZWJ / emoji)。复制路径尤其要走 `Intl.Segmenter` 守门。
- **东亚宽度**:列宽 / 光标定位要按 east-asian width(全角占 2 列),不得假设每字符 1 列。
- **IME composition**:键盘处理必须拦截输入法组合态 —— 检查 `event.isComposing` 或 `keyCode === 229`,
  组合期间不得把按键透传给 PTY。
- **locale**:不得依赖进程默认 locale(GUI 启动可能丢 `LANG` → C locale 致中文乱码)。
  根因已在 `main()` 最早期进程级兜 `LC_CTYPE=UTF-8`,子进程继承;不要逐 spawn 打补丁、不要移除该兜底。

## 严重级

撕裂字素 / 漏 IME 拦截 / 破坏 locale 兜底 → HIGH(合并前应修)。前端 CJK 现状为 A+,改动须保持。
