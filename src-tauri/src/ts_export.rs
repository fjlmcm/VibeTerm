//! TS 镜像生成:从 Rust 类型导出 `web/packages/ipc-types/src/generated.ts`。
//!
//! 用法:
//!   - CI / 常规测试:`cargo test --bin vibeterm ts_mirror` —— 生成结果与磁盘文件**逐字节比对**,
//!     不一致即红(= 有人改了 Rust 类型没重新生成)。
//!   - 重新生成:`VIBETERM_UPDATE_TS=1 cargo test --bin vibeterm ts_mirror`。
//!
//! 手写的补充类型(事件 payload / 别名)保留在 ipc-types/src/index.ts,re-export generated。

#[cfg(test)]
mod tests {
    use specta_typescript::{BigIntExportBehavior, Typescript};

    fn build_types() -> specta::TypeCollection {
        let mut t = specta::TypeCollection::default();
        // vibeterm-ipc(schema crate)
        t.register::<vibeterm_ipc::TaskDto>();
        t.register::<vibeterm_ipc::TaskStatus>();
        t.register::<vibeterm_ipc::TaskLocation>();
        t.register::<vibeterm_ipc::SplitNode>();
        t.register::<vibeterm_ipc::Orientation>();
        t.register::<vibeterm_ipc::WorktreeRef>();
        t.register::<vibeterm_ipc::CreateTaskOpts>();
        t.register::<vibeterm_ipc::BranchSpecDto>();
        t.register::<vibeterm_ipc::SpawnPtyOpts>();
        t.register::<vibeterm_ipc::SpawnPtyResult>();
        t.register::<vibeterm_ipc::IpcError>();
        // vibeterm-config
        t.register::<vibeterm_config::Theme>();
        t.register::<vibeterm_config::Config>();
        t.register::<vibeterm_config::EnvFile>();
        t.register::<vibeterm_config::KeybindingsFile>();
        t.register::<vibeterm_config::PromptsFile>();
        t.register::<vibeterm_config::actions::ActionsFile>();
        t.register::<vibeterm_config::NotifyFile>();
        t.register::<vibeterm_config::StatusLineFile>();
        t.register::<vibeterm_config::LayoutTemplate>();
        // vibeterm-agent-watch
        t.register::<vibeterm_agent_watch::UsageCache>();
        t.register::<vibeterm_agent_watch::ClaudeSession>();
        t.register::<vibeterm_agent_watch::CodexSnapshot>();
        t.register::<vibeterm_agent_watch::claude::blocks::ActiveBlock>();
        t.register::<vibeterm_agent_watch::stats::UsageStats>();
        t.register::<vibeterm_agent_watch::claude::pricing::PricingStatus>();
        t.register::<vibeterm_agent_watch::provider::AgentUsage>();
        // vibeterm-status / git
        t.register::<vibeterm_status::AgentKind>();
        t.register::<vibeterm_git::WorktreeStatus>();
        // bin 内 DTO
        t.register::<crate::CliStatus>();
        t.register::<crate::ResumeInfo>();
        t.register::<crate::AppUpdateInfo>();
        t.register::<crate::GitDiffResult>();
        t.register::<crate::NotifySoundData>();
        t.register::<crate::BuiltinSound>();
        t.register::<crate::ExecuteActionResult>();
        t.register::<crate::DetectAgentResult>();
        t
    }

    #[test]
    fn ts_mirror_generated_is_up_to_date() {
        let types = build_types();
        let header = "// 本文件由 Rust 类型自动生成 —— 不要手改。\n\
                      // 重新生成:src-tauri 下 `VIBETERM_UPDATE_TS=1 cargo test --bin vibeterm ts_mirror`\n";
        let out: String = Typescript::default()
            .bigint(BigIntExportBehavior::Number)
            .header(header)
            .export(&types)
            .expect("specta export");
        // 后处理(统一规则):specta rc 把「serde(default)」字段一律标可选,但本项目里
        // default(无 skip_serializing_if)的非 Option 字段**序列化恒输出**,wire 必有——
        // default 仅为读老配置文件兜底。规则:可选字段的类型不含 `null`(= 非 Option)→ 去掉 `?`。
        // Option 字段(类型含 null,可能配 skip 真缺省)保持原样。
        // 已知例外:StatusLineItemDetail.metadata(HashMap + skip_if_empty,wire 可缺省但 specta
        // 未标 ?)由前端 statusLineItemDetail helper 归一(ipc-types/index.ts)。
        // (specta 的 #[specta(optional = false)] 在 rc.18 宏里被 serde 属性覆盖,不可用。)
        let re = regex::Regex::new(r"(\w+)\?: ([^;\n]+?)( ?[;}])").expect("re");
        let out = re
            .replace_all(&out, |c: &regex::Captures<'_>| {
                if c[2].contains("null") {
                    c[0].to_string()
                } else {
                    format!("{}: {}{}", &c[1], &c[2], &c[3])
                }
            })
            .into_owned();
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../web/packages/ipc-types/src/generated.ts"
        );
        if std::env::var("VIBETERM_UPDATE_TS").is_ok() {
            std::fs::write(path, &out).expect("write generated.ts");
            return;
        }
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        assert_eq!(
            existing, out,
            "generated.ts 与 Rust 类型不同步:运行 VIBETERM_UPDATE_TS=1 cargo test --bin vibeterm ts_mirror 重新生成"
        );
    }
}
