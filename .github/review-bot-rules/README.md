# Review Bot Rules — VibeTerm 红线的机器可执行版

这些规则把 `CLAUDE.md` 的 5 条红线 + 若干工程约束落成自动 code review 的检查项,
供 CodeRabbit(`.coderabbit.yaml`)等机器审查器引用,使红线不再仅靠人记。

借鉴 cmux 的 `.github/review-bot-rules/` 模式:规则文件是唯一真相,
`.coderabbit.yaml` 的 `path_instructions` 内联其要点并按路径作用域生效。

| 规则 | 严重级 | 作用域 |
|---|---|---|
| [zero-intrusion](zero-intrusion.md) | CRITICAL | Rust / 全仓 |
| [config-isolation](config-isolation.md) | CRITICAL | Rust 测试 / 配置 |
| [cjk-first-class](cjk-first-class.md) | HIGH | 前端终端/文本 |
| [i18n-completeness](i18n-completeness.md) | HIGH | 前端 i18n |
| [theme-ansi-immutable](theme-ansi-immutable.md) | MEDIUM | 主题/配色 |
| [no-hardcoded-real-paths](no-hardcoded-real-paths.md) | HIGH | 测试/示例 |

> 严重级对应 `CLAUDE.md` 与 `rules/common/code-review.md`:CRITICAL=阻止合并,HIGH=合并前应修,MEDIUM=考虑修。
