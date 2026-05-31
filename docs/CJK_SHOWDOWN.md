# CJK Terminal Showdown

> **目的**:用一组可复现的测试用例,公开对比主流 AI 终端在中日韩(CJK)
> 场景下的表现。给中日韩开发者一个**事实判断依据**,而不是听任何一方
> (包括 VibeTerm)的宣传。
>
> **结论(本文截稿时)**:**没有一个英文一线 AI 终端把 CJK 当 P0 做**。
> VibeTerm 把这件事做到了一等公民。欢迎 PR 补充证据 / 纠正打分。

---

## 一、为什么 CJK 是 AI 终端的盲区

2025-2026 主流 AI 终端 / CLI 在 CJK 上的真实情况:

| 工具 | 已知 CJK Issue | 状态 |
|---|---|---|
| Claude Code | [#1547 日文 IME 卡顿 + 双候选窗](https://github.com/anthropics/claude-code/issues/1547) (241 👍) | Open |
| Claude Code | [#8405 IME 合成中 Enter 误发](https://github.com/anthropics/claude-code/issues/8405) (95 👍) | Closed 但未根治 |
| Claude Code | [#45508 流式 UTF-8 边界破坏 CJK](https://github.com/anthropics/claude-code/issues/45508) | Open |
| Claude Code | [#13438 CJK 表格错位](https://github.com/anthropics/claude-code/issues/13438) | Open |
| Claude Code | [#14812 中文换行截断回归](https://github.com/anthropics/claude-code/issues/14812) | Open |
| Claude Code | [#7332 中文输出乱码](https://github.com/anthropics/claude-code/issues/7332) | Open |
| Claude Code | [#19207 IME 光标位置错误](https://github.com/anthropics/claude-code/issues/19207) | Open |
| Claude Code | [#14597 韩文 + 框线截断](https://github.com/anthropics/claude-code/issues/14597) | Open |
| Claude Code | [#43170 Companion bubble 截断 CJK](https://github.com/anthropics/claude-code/issues/43170) | Open |
| Warp | [#9357 无简中 UI](https://github.com/warpdotdev/warp/issues/9357) | Open(跨 2025-2026 未修) |
| Warp | [#7436 sidebar 中文文件名乱码](https://github.com/warpdotdev/warp/issues/7436) | Open |
| Warp | [#6891 中文 IME issue](https://github.com/warpdotdev/warp/issues/6891) | Open |
| cmux | [#4519 强行注入字体,中文用户无法覆盖](https://github.com/manaflow-ai/cmux/issues/4519) | Open |
| cmux | [#1693 韩文字体被注入](https://github.com/manaflow-ai/cmux/issues/1693) | Open |
| cmux | [#2755 日文字体被注入](https://github.com/manaflow-ai/cmux/issues/2755) | Open |
| Ghostty | [#12173 macOS 中文标点定位 bug](https://github.com/ghostty-org/ghostty/issues/12173) | Open(1.3 改善了部分) |
| xterm.js | [#1059 / #467 / #4063 wcwidth 历史 bug 链](https://github.com/xtermjs/xterm.js/issues/1059) | Partial |
| Microsoft Terminal | [#370 CJK ambiguous width](https://github.com/microsoft/terminal/issues/370) | Open |
| Microsoft Terminal | [#7955 后台活动通知(5 年 spec 无人实现)](https://github.com/microsoft/terminal/issues/7955) | Open |

> "**每一个 AI 终端项目仓库都有 CJK 长期未修的 issue**,但被英文用户的 P0
> bug 持续覆盖。这是中日韩开发者市场的结构性空缺。" — VibeTerm 调研结论

---

## 二、6 项测试用例(可复制粘贴执行)

在任何 AI 终端里跑这 6 项测试,记录通过 / 失败。

### Test 1:中文文件名 / 路径渲染

```sh
mkdir -p /tmp/cjk-test
cd /tmp/cjk-test
touch 测试文件.txt
ls -la
```

**期望**:`测试文件.txt` 完整渲染、对齐正确、文件名宽度 = 5 个汉字 + 4 个英文字符 = 14 cell。

**常见失败**:文件名被截、`?` 乱码、列错位、tab 补全无法触发。

---

### Test 2:中日韩文本 echo

```sh
echo "中文 日本語 한국어 — 测试混排"
echo "𠀀 𠁆 𠂉"   # 4 字节 surrogate pair 汉字(超出 BMP)
echo "👨‍👩‍👧‍👦 family ZWJ sequence"
```

**期望**:
- 三种文字正确显示、不破坏行宽
- 4 字节汉字 (`𠀀` 等)正常显示,不丢字
- ZWJ emoji 合成,不被拆成多个 cell

**常见失败**:`???` 乱码、半字、ZWJ 分裂、超出 BMP 的字直接消失。

---

### Test 3:IME 候选框定位

(macOS:用搜狗 / 系统拼音;Windows:微软拼音;Linux:fcitx / ibus)

1. 在终端里输入 prompt `claude` 或 `aider` 进入 agent
2. 启动 IME,输入拼音 `nihaoshijie`
3. 观察候选框位置 — 应在光标正下方
4. 选词时键盘上下键 — 不应被吞掉送到 PTY
5. 候选窗口选完按 Enter 确认 — 应只送选中文本,**不应同时触发 agent 提交 prompt**

**常见失败**(`claude-code#8405` 高赞):
- 候选窗悬空在屏幕中间 / 光标位置错
- 候选 Enter 误把当前输入框内容提交给 agent(灾难性)
- 双候选窗(`claude-code#1547`,241 👍)

---

### Test 4:复制粘贴中日韩字

1. 终端输出 `echo "中文混排 abc 中文 def"`
2. 鼠标选中输出中包含 "中文" 的一段
3. Cmd+C 复制
4. 粘贴到记事本 / 文本编辑器

**期望**:
- 选中范围按 grapheme 取整(不出现半个汉字 / 半个 emoji ZWJ)
- 粘贴后字符完整、无 `?`、无 lone surrogate

**常见失败**:复制出 `中?`、空格被吞、emoji ZWJ 拆开。

---

### Test 5:中文 prompt + agent 长输出

(需要装 `claude` / `aider` / `gemini` / `cursor-agent` 之一)

```
claude
> 帮我写一个 Python 程序输出"你好世界"
```

agent 输出后:
1. 滚动回看历史 — 中文行不应错位
2. 调整窗口宽度 — 中文 reflow 不应丢字
3. 跨 chunk 流式输出时(大段中文 LLM 流式生成)— 不应 `���` 边界乱码

**常见失败**(`claude-code#45508`):流式输出时 UTF-8 多字节字符在 chunk
边界被切开,渲染 `���`。

---

### Test 6:中文界面 / 菜单 / 通知

1. 打开终端的设置 / 菜单栏
2. 查找是否有简中 / 繁中 / 日 / 韩界面选项

**期望**:四种语言至少有简中或日文(目标用户人口最多的两个)。

**常见失败**:菜单只有英文,所有教程都要用户先看一遍英文文档。

---

## 三、评分维度

每项测试评分:
- **PASS** ✅ — 完全符合期望
- **PARTIAL** ⚠️ — 大部分对,有 1-2 个边缘 case 失败
- **FAIL** ❌ — 主流路径就失败

| 维度 | 权重 | 说明 |
|---|---|---|
| 渲染正确性 | 30% | 文件名 / echo / agent 输出不能错位、乱码 |
| 输入正确性 | 30% | IME 候选窗、Enter 误发、上下键 |
| 复制粘贴 | 15% | grapheme cluster 切片正确 |
| 中文界面 | 10% | 至少有简中 UI / 文档 |
| 跨平台一致性 | 10% | macOS / Windows / Linux 一致 |
| 维护活跃度 | 5% | 已知 CJK issue 是否被 triage |

---

## 四、对比矩阵(社区填表中)

> 注:**VibeTerm 的"PASS"是自评 + 等待社区复现验证**;其他工具
> 的"FAIL"基于上述公开 GitHub issue + 实测。欢迎 PR 修正。

| Tool          | T1 文件名 | T2 echo | T3 IME | T4 复制 | T5 流式 | T6 中文 UI | 备注 |
|---|---|---|---|---|---|---|---|
| **VibeTerm**  | PASS ✅   | PASS ✅ | PASS ✅ | PASS ✅ | PASS ✅ | PASS ✅ (zh-CN/en/ja) | xterm.js + Unicode 15 graphemes + IME composition 拦截 + Intl.Segmenter 复制守门 |
| Claude Code   | PARTIAL ⚠️ | FAIL ❌ | FAIL ❌ | ?      | FAIL ❌ | PARTIAL ⚠️ (2.1+ 才加 Language) | 9 个未修 CJK issue |
| Warp          | FAIL ❌   | PASS ✅ | FAIL ❌ | PASS ✅ | PASS ✅ | FAIL ❌ (无简中) | #6891/#7436/#9357 跨年未修 |
| cmux          | PASS ✅   | PARTIAL ⚠️ | ?    | PASS ✅ | PASS ✅ | FAIL ❌ (字体注入硬伤) | #4519/#1693/#2755 |
| Ghostty       | PASS ✅   | PASS ✅ | PARTIAL ⚠️ | PASS ✅ | PASS ✅ | FAIL ❌ | macOS 中文标点 #12173 |
| Wave          | PASS ✅   | PASS ✅ | ?      | PASS ✅ | PASS ✅ | FAIL ❌ (英文文档) | 走"全能工作站"路线 |
| Terax         | PASS ✅   | PASS ✅ | ?      | PASS ✅ | PASS ✅ | FAIL ❌ (英文 only) | 同栈对手,定位发散 |

---

## 五、复现 VibeTerm 评分

```bash
# 1. 克隆并跑起来
git clone https://github.com/fjlmcm/VibeTerm
cd VibeTerm
pnpm install
pnpm tauri dev

# 2. 把上面 6 项测试逐项跑一遍
# 3. 如果发现任何 PARTIAL / FAIL, 提 issue:
#    https://github.com/fjlmcm/VibeTerm/issues/new
```

---

## 六、技术实现说明(VibeTerm 怎么做对的)

| 测试 | 实现 |
|---|---|
| T1 文件名 / T2 echo / T5 流式 | xterm.js + `@xterm/addon-unicode-graphemes` v0.4.0 + `term.unicode.activeVersion = "15-graphemes"`(Unicode 15 宽字符表) |
| T3 IME | `term.attachCustomKeyEventHandler` 返回 `e.isComposing \|\| keyCode==229` 时为 false → xterm.js 跳过 keydown,IME 完整接手合成。`onWinKeydown` 同步加 isComposing 短路 |
| T4 复制 | 右键菜单 Copy 路径走 `Intl.Segmenter({granularity:"grapheme"})` 重组,自动丢弃 lone surrogate / 半截 ZWJ |
| T6 中文 UI | i18n zh-CN / en / ja 三语,顶栏菜单 / 右键菜单 / Dialog 全部本地化(macOS PredefinedMenuItem 标签也覆盖) |

---

## 七、贡献

发现新 CJK bug / 完成 PR 修复 / 跑测试套件填表:
- Issue: [VibeTerm Issues](https://github.com/fjlmcm/VibeTerm/issues)
- PR: 本文件直接接收对比矩阵更新 / 测试用例增补

**判断标准**:任何一项从 PASS 降级,需附复现命令 + 截图 / 录屏。
任何一项从 FAIL/PARTIAL 升 PASS,需附 commit + 测试。
