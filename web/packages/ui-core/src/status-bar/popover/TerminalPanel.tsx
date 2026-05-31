// status-bar/popover/TerminalPanel.tsx — 终端段 (task / cwd / git / pr).
//
// 不复述状态栏 widget 的展示, 显示 glance 之外的深度信息:
//   - task: 名 / status / worktree path / agent kind
//   - cwd: 全路径 mono
//   - git: branch / commit hash 12 字符 / ahead-behind / 精确 changes / stash
//   - pr: status 字符串

import { Show, type Component } from "solid-js";
import { t } from "../../i18n";
import type { RenderContext } from "../widgets";
import { Row, Section } from "./atoms";

export const TerminalPanel: Component<{ ctx: RenderContext }> = (props) => {
  const { cwd, git, gitStashCount, prStatus, task } = props.ctx;
  const kind = props.ctx.agentKind;
  return (
    <>
      <Show when={task()}>
        {(tk) => (
          <Section title={t("statusbar.popover.task")}>
            <Row label={t("statusbar.popover.task")} value={tk().name} />
            <Row label={t("statusbar.popover.status")} value={tk().status} />
            <Show when={tk().worktree}>
              <Row
                label={t("statusbar.popover.worktree")}
                value={`${tk().worktree!.worktree_path.split("/").filter(Boolean).pop() ?? ""}${
                  tk().worktree!.branch ? ` · ${tk().worktree!.branch}` : ""
                }`}
                mono
              />
            </Show>
            <Show when={kind()}>
              <Row label={t("statusbar.popover.agent")} value={kind()!} />
            </Show>
          </Section>
        )}
      </Show>

      <Show when={cwd() || git()}>
        <Section title={t("statusbar.popover.cwd")}>
          <Show when={cwd()}>
            <Row label={t("statusbar.popover.cwd")} value={cwd()!} mono />
          </Show>
          <Show when={git()?.branch}>
            <Row label={t("statusbar.popover.branch")} value={git()!.branch!} mono />
          </Show>
          <Show when={git()?.head}>
            <Row label={t("statusbar.popover.commit")} value={git()!.head.slice(0, 12)} mono />
          </Show>
          <Show when={git() && (git()!.ahead > 0 || git()!.behind > 0)}>
            <Row
              label={t("statusbar.popover.tracking")}
              value={`${git()!.ahead > 0 ? `↑${git()!.ahead}` : ""}${
                git()!.behind > 0 ? ` ↓${git()!.behind}` : ""
              }`.trim()}
            />
          </Show>
          <Show
            when={
              git() &&
              ((git()!.staged ?? 0) > 0 ||
                (git()!.unstaged ?? 0) > 0 ||
                (git()!.untracked ?? 0) > 0)
            }
            fallback={
              <Show when={git()?.branch}>
                <Row
                  label={t("statusbar.popover.changes")}
                  value={t("statusbar.popover.no_changes")}
                />
              </Show>
            }
          >
            <Row
              label={t("statusbar.popover.changes")}
              value={[
                (git()!.staged ?? 0) > 0 ? `staged ${git()!.staged}` : null,
                (git()!.unstaged ?? 0) > 0 ? `unstaged ${git()!.unstaged}` : null,
                (git()!.untracked ?? 0) > 0 ? `untracked ${git()!.untracked}` : null,
              ]
                .filter(Boolean)
                .join(" · ")}
            />
          </Show>
          <Show when={gitStashCount() > 0}>
            <Row label={t("statusbar.popover.stash")} value={String(gitStashCount())} />
          </Show>
          <Show when={prStatus()}>
            <Row label={t("statusbar.popover.pr")} value={prStatus()!} />
          </Show>
        </Section>
      </Show>
    </>
  );
};
