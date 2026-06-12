// 任务管理模态(隐私 tab 风格统一 / L1 worktree 挂载)
//
// 用 SolidJS 自渲模态替掉 prompt() / confirm():
//   - macOS WKWebView 默认禁用这两个原生对话框 → 按钮无响应
//   - 自渲可以贴主题 / i18n / 嵌入更复杂内容(close 模态展示终端数 / 等待数)
//
// 行为:
//   - ESC 关
//   - 点 backdrop 关(传 onClose)
//   - autofocus input(NewTaskDialog)
//   - Enter 提交(L1:勾了 worktree 后改成 Cmd+Enter,避免在 branch 字段误触发)

import { Show, createSignal, createEffect, onMount, type Component } from "solid-js";
import { Folder, FolderOpen, GitBranch, Check, X as XIcon } from "lucide-solid";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { t, ipc } from "@vibeterm/ui-core";
import type { BranchSpec, TaskDto, WorktreeRef } from "@vibeterm/ipc-types";

/** L1:勾选挂 worktree 时的表单值 */
export interface WorktreeFormValue {
  repoPath: string;
  branchMode: "existing" | "new_from_head";
  branch: string;
  worktreePath: string;
}

export interface NewTaskDialogProps {
  onClose: () => void;
  /**
   * L1:如果用户勾了 worktree,会先 `git worktree add` 再 createTask({name, cwd: null, worktree})。
   * 失败时通过 onError 上抛(对话框不关)。
   */
  onSubmit: (name: string, cwd: string | null, worktree: WorktreeRef | null) => void;
}

export const NewTaskDialog: Component<NewTaskDialogProps> = (props) => {
  const [name, setName] = createSignal("");
  const [cwd, setCwd] = createSignal("");
  const [attachWt, setAttachWt] = createSignal(false);
  const [repoPath, setRepoPath] = createSignal("");
  const [repoValid, setRepoValid] = createSignal<null | boolean>(null);
  const [branches, setBranches] = createSignal<string[]>([]);
  const [branchMode, setBranchMode] = createSignal<"existing" | "new_from_head">("new_from_head");
  const [branch, setBranch] = createSignal("");
  const [worktreePath, setWorktreePath] = createSignal("");
  const [worktreePathTouched, setWorktreePathTouched] = createSignal(false);
  const [submitting, setSubmitting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let inputEl: HTMLInputElement | undefined;

  onMount(() => {
    setTimeout(() => inputEl?.focus(), 0);
  });

  // repoPath 失焦或变化时 → 校验 + 拉分支
  const validateRepo = async () => {
    const p = repoPath().trim();
    if (!p) {
      setRepoValid(null);
      setBranches([]);
      return;
    }
    try {
      const ok = await ipc.gitIsRepo(p);
      setRepoValid(ok);
      if (ok) {
        try {
          const root = await ipc.gitRepoRoot(p);
          if (root && root !== p) setRepoPath(root);
        } catch {
          // 忽略,继续用用户输入的 path
        }
        try {
          setBranches(await ipc.gitListBranches(repoPath()));
        } catch {
          setBranches([]);
        }
      } else {
        setBranches([]);
      }
    } catch {
      setRepoValid(false);
      setBranches([]);
    }
  };

  // 自动建议 worktree path:<repo>/.worktrees/<branch>
  // 用户手动改过后(touched)不再覆盖
  createEffect(() => {
    if (worktreePathTouched()) return;
    const r = repoPath().trim();
    const b = branch().trim();
    if (r && b && repoValid() === true) {
      setWorktreePath(`${r.replace(/\/+$/, "")}/.worktrees/${b}`);
    } else {
      setWorktreePath("");
    }
  });

  const canSubmit = () => {
    if (!name().trim()) return false;
    if (!attachWt()) return true;
    if (repoValid() !== true) return false;
    if (!branch().trim()) return false;
    if (!worktreePath().trim()) return false;
    return !submitting();
  };

  const submit = async () => {
    if (!canSubmit()) return;
    const n = name().trim();
    if (!attachWt()) {
      props.onSubmit(n, cwd().trim() || null, null);
      return;
    }
    // 挂 worktree:先 git_add_worktree,拿到 WorktreeRef 再交给上层 createTask
    setSubmitting(true);
    setError(null);
    try {
      const spec: BranchSpec =
        branchMode() === "existing"
          ? { mode: "existing", branch: branch().trim() }
          : { mode: "new_from_head", branch: branch().trim() };
      const wt = await ipc.gitAddWorktree(repoPath().trim(), worktreePath().trim(), spec);
      props.onSubmit(n, null, wt);
    } catch (e) {
      setError(ipc.formatIpcError(e));
      setSubmitting(false);
    }
  };

  return (
    <Backdrop onClose={submitting() ? () => {} : props.onClose}>
      <div
        data-testid="new-task-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          // 🔴 红线4:IME 组合态下,回车=确认候选词、Esc=取消候选,都不应触发对话框提交/关闭。
          // 否则中文/日文等用户敲回车选词时会被误当"提交"而直接建任务。
          if (e.isComposing || e.keyCode === 229) return;
          if (e.key === "Escape" && !submitting()) props.onClose();
          // L1:勾了 worktree 用 Cmd+Enter 提交,避免误触发
          if (e.key === "Enter") {
            if (!attachWt() || e.metaKey || e.ctrlKey) submit();
          }
        }}
        style={{
          background: "var(--color-surface)",
          border: "1px solid var(--color-border)",
          "border-radius": "10px",
          width: "480px",
          padding: "20px 24px",
          "box-shadow": "0 12px 32px rgba(0,0,0,0.5)",
          display: "flex",
          "flex-direction": "column",
          gap: "14px",
          color: "var(--color-text)",
        }}
      >
        <h2 style={{ margin: 0, "font-size": "15px", "font-weight": 600 }}>
          {t("dialog.new_task.title")}
        </h2>
        <label style={labelStyle()}>
          {t("dialog.new_task.name_label")}
          <input
            ref={inputEl}
            data-testid="new-task-name"
            value={name()}
            onInput={(e) => setName(e.currentTarget.value)}
            placeholder={t("dialog.new_task.name_placeholder")}
            style={inputStyle()}
          />
        </label>

        <Show when={!attachWt()}>
          <label style={labelStyle()}>
            <span style={{ display: "flex", "align-items": "center", gap: "6px" }}>
              <Folder size={12} />
              {t("dialog.new_task.cwd_label")}
            </span>
            <div style={{ display: "flex", gap: "4px" }}>
              <input
                data-testid="new-task-cwd"
                value={cwd()}
                onInput={(e) => setCwd(e.currentTarget.value)}
                placeholder="~/projects/foo"
                style={{ ...inputStyle(), flex: 1 }}
              />
              <BrowseBtn testid="new-task-cwd-browse" onPick={setCwd} />
            </div>
          </label>
        </Show>

        {/* L1 worktree section */}
        <label
          style={{
            display: "flex",
            "align-items": "center",
            gap: "8px",
            "font-size": "12px",
            color: "var(--color-text)",
            cursor: "pointer",
            "user-select": "none",
          }}
        >
          <input
            type="checkbox"
            data-testid="new-task-attach-wt"
            checked={attachWt()}
            onChange={(e) => setAttachWt(e.currentTarget.checked)}
          />
          <GitBranch size={13} />
          {t("dialog.new_task.attach_worktree")}
        </label>

        <Show when={attachWt()}>
          <div
            data-testid="new-task-wt-section"
            style={{
              display: "flex",
              "flex-direction": "column",
              gap: "10px",
              padding: "12px",
              background: "var(--color-bg)",
              border: "1px solid var(--color-border)",
              "border-radius": "6px",
            }}
          >
            <label style={labelStyle()}>
              <span style={{ display: "flex", "align-items": "center", gap: "6px" }}>
                {t("dialog.new_task.repo_label")}
                <Show when={repoValid() === true}>
                  <Check size={12} style={{ color: "var(--color-status-running)" }} />
                </Show>
                <Show when={repoValid() === false}>
                  <XIcon size={12} style={{ color: "var(--color-status-waiting)" }} />
                </Show>
              </span>
              <div style={{ display: "flex", gap: "4px" }}>
                <input
                  data-testid="new-task-repo-path"
                  value={repoPath()}
                  onInput={(e) => setRepoPath(e.currentTarget.value)}
                  onBlur={validateRepo}
                  placeholder="~/projects/foo"
                  style={{ ...inputStyle(), flex: 1 }}
                />
                <BrowseBtn
                  testid="new-task-repo-path-browse"
                  onPick={(p) => {
                    setRepoPath(p);
                    validateRepo();
                  }}
                />
              </div>
            </label>

            <div style={{ display: "flex", gap: "16px", "font-size": "12px" }}>
              <label style={{ display: "flex", "align-items": "center", gap: "6px", cursor: "pointer" }}>
                <input
                  type="radio"
                  name="branch-mode"
                  data-testid="branch-mode-new"
                  checked={branchMode() === "new_from_head"}
                  onChange={() => setBranchMode("new_from_head")}
                />
                {t("dialog.new_task.branch_mode_new")}
              </label>
              <label style={{ display: "flex", "align-items": "center", gap: "6px", cursor: "pointer" }}>
                <input
                  type="radio"
                  name="branch-mode"
                  data-testid="branch-mode-existing"
                  checked={branchMode() === "existing"}
                  onChange={() => setBranchMode("existing")}
                />
                {t("dialog.new_task.branch_mode_existing")}
              </label>
            </div>

            <label style={labelStyle()}>
              {t("dialog.new_task.branch_label")}
              <input
                data-testid="new-task-branch"
                value={branch()}
                onInput={(e) => setBranch(e.currentTarget.value)}
                list="vt-branches-datalist"
                placeholder={branchMode() === "new_from_head" ? "feature/foo" : "main"}
                style={inputStyle()}
              />
              <datalist id="vt-branches-datalist">
                {branches().map((b) => (
                  <option value={b} />
                ))}
              </datalist>
            </label>

            <label style={labelStyle()}>
              {t("dialog.new_task.worktree_path_label")}
              <div style={{ display: "flex", gap: "4px" }}>
                <input
                  data-testid="new-task-wt-path"
                  value={worktreePath()}
                  onInput={(e) => {
                    setWorktreePath(e.currentTarget.value);
                    setWorktreePathTouched(true);
                  }}
                  placeholder="<repo>/.worktrees/<branch>"
                  style={{ ...inputStyle(), flex: 1 }}
                />
                <BrowseBtn
                  testid="new-task-wt-path-browse"
                  onPick={(p) => {
                    setWorktreePath(p);
                    setWorktreePathTouched(true);
                  }}
                />
              </div>
            </label>

            <Show when={error()}>
              <div
                data-testid="new-task-wt-error"
                style={{
                  "font-size": "12px",
                  color: "var(--color-status-waiting)",
                  "white-space": "pre-wrap",
                  "word-break": "break-all",
                }}
              >
                {error()}
              </div>
            </Show>
          </div>
        </Show>

        <div style={{ display: "flex", "justify-content": "flex-end", gap: "8px", "margin-top": "8px" }}>
          <button
            data-testid="new-task-cancel"
            style={btnStyle(false)}
            onClick={props.onClose}
            disabled={submitting()}
          >
            {t("dialog.cancel")}
          </button>
          <button
            data-testid="new-task-submit"
            style={btnStyle(true)}
            onClick={submit}
            disabled={!canSubmit()}
          >
            {submitting() ? t("dialog.new_task.submitting") : t("dialog.new_task.submit")}
          </button>
        </div>
      </div>
    </Backdrop>
  );
};

export interface ConfirmCloseDialogProps {
  task: TaskDto;
  /** 该任务下所有终端的 status,用于摘要 */
  terminalStatuses: ("idle" | "running" | "waiting_input")[];
  onCancel: () => void;
  /** 确认关闭。`removeWorktree` 仅 task.worktree 存在时由 UI 询问后传 true */
  onConfirm: (removeWorktree: boolean, force: boolean) => void;
}

/** 通用 "浏览目录" 按钮 — 弹 native folder picker, 选中后回填. */
const BrowseBtn: Component<{ testid: string; onPick: (p: string) => void }> = (p) => (
  <button
    type="button"
    data-testid={p.testid}
    title={t("dialog.browse")}
    onClick={async () => {
      try {
        const picked = await openDialog({
          directory: true,
          multiple: false,
        });
        if (typeof picked === "string" && picked) p.onPick(picked);
      } catch (e) {
        console.error("[dialog] pick directory failed", e);
      }
    }}
    style={{
      background: "var(--color-surface)",
      color: "var(--color-text)",
      border: "1px solid var(--color-border)",
      "border-radius": "4px",
      padding: "0 10px",
      cursor: "pointer",
      display: "flex",
      "align-items": "center",
      gap: "4px",
      "font-size": "12px",
    }}
  >
    <FolderOpen size={12} />
  </button>
);

export const ConfirmCloseDialog: Component<ConfirmCloseDialogProps> = (props) => {
  const totalCount = () => props.terminalStatuses.length;
  const waitingCount = () =>
    props.terminalStatuses.filter((s) => s === "waiting_input").length;
  const runningCount = () =>
    props.terminalStatuses.filter((s) => s === "running").length;
  const hasWorktree = () => !!props.task.worktree;
  const wtDirty = () => !!props.task.worktree?.is_dirty;
  const [removeWt, setRemoveWt] = createSignal(false);
  const [force, setForce] = createSignal(false);

  return (
    <Backdrop onClose={props.onCancel}>
      <div
        data-testid="confirm-close-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") props.onCancel();
          if (e.key === "Enter") props.onConfirm(removeWt(), force());
        }}
        style={{
          background: "var(--color-surface)",
          border: "1px solid var(--color-border)",
          "border-radius": "10px",
          width: "460px",
          padding: "20px 24px",
          "box-shadow": "0 12px 32px rgba(0,0,0,0.5)",
          display: "flex",
          "flex-direction": "column",
          gap: "12px",
          color: "var(--color-text)",
        }}
      >
        <h2 style={{ margin: 0, "font-size": "15px", "font-weight": 600 }}>
          {t("dialog.confirm_close.title", { name: props.task.name })}
        </h2>
        <Show when={totalCount() > 0} fallback={
          <p style={{ margin: 0, "font-size": "13px", color: "var(--color-text-2)" }}>
            {t("dialog.confirm_close.no_terminals")}
          </p>
        }>
          <p style={{ margin: 0, "font-size": "13px", color: "var(--color-text-2)", "line-height": 1.6 }}>
            {t("dialog.confirm_close.summary_prefix", { count: totalCount() })}
            <Show when={waitingCount() > 0}>
              <span
                data-testid="confirm-close-waiting-count"
                style={{ color: "var(--color-status-waiting)", "font-weight": 600 }}
              >
                {t("dialog.confirm_close.waiting", { count: waitingCount() })}
              </span>
            </Show>
            <Show when={waitingCount() === 0 && runningCount() > 0}>
              <span style={{ color: "var(--color-status-running)" }}>
                {t("dialog.confirm_close.running", { count: runningCount() })}
              </span>
            </Show>
          </p>
        </Show>

        <Show when={hasWorktree()}>
          <div
            data-testid="confirm-close-wt-section"
            style={{
              "font-size": "12px",
              padding: "10px 12px",
              background: "var(--color-bg)",
              border: "1px solid var(--color-border)",
              "border-radius": "6px",
              display: "flex",
              "flex-direction": "column",
              gap: "8px",
            }}
          >
            <div style={{ color: "var(--color-text-2)" }}>
              <GitBranch size={11} style={{ display: "inline", "margin-right": "4px" }} />
              {props.task.worktree?.branch ?? "(detached)"} · {props.task.worktree?.worktree_path}
            </div>
            <label style={{ display: "flex", "align-items": "center", gap: "6px", cursor: "pointer" }}>
              <input
                type="checkbox"
                data-testid="confirm-close-remove-wt"
                checked={removeWt()}
                onChange={(e) => setRemoveWt(e.currentTarget.checked)}
              />
              {t("dialog.confirm_close.remove_worktree")}
            </label>
            <Show when={removeWt() && wtDirty()}>
              <label
                style={{
                  display: "flex",
                  "align-items": "center",
                  gap: "6px",
                  cursor: "pointer",
                  color: "var(--color-status-waiting)",
                }}
              >
                <input
                  type="checkbox"
                  data-testid="confirm-close-force"
                  checked={force()}
                  onChange={(e) => setForce(e.currentTarget.checked)}
                />
                {t("dialog.confirm_close.force_dirty")}
              </label>
            </Show>
          </div>
        </Show>

        <div style={{ display: "flex", "justify-content": "flex-end", gap: "8px", "margin-top": "8px" }}>
          <button
            data-testid="confirm-close-cancel"
            autofocus
            style={btnStyle(false)}
            onClick={props.onCancel}
          >
            {t("dialog.cancel")}
          </button>
          <button
            data-testid="confirm-close-submit"
            style={{
              ...btnStyle(true),
              background: "var(--color-status-waiting)",
              "border-color": "var(--color-status-waiting)",
            }}
            onClick={() => props.onConfirm(removeWt(), force())}
            disabled={removeWt() && wtDirty() && !force()}
          >
            {t("dialog.confirm_close.submit")}
          </button>
        </div>
      </div>
    </Backdrop>
  );
};

const Backdrop: Component<{ onClose: () => void; children: any }> = (p) => (
  <div
    onClick={p.onClose}
    style={{
      position: "fixed",
      inset: 0,
      background: "rgba(0,0,0,0.5)",
      display: "flex",
      "justify-content": "center",
      "align-items": "center",
      "z-index": 2100,
    }}
  >
    {p.children}
  </div>
);

function labelStyle() {
  return {
    display: "flex",
    "flex-direction": "column" as const,
    gap: "6px",
    "font-size": "12px",
    color: "var(--color-text-2)",
  };
}

function inputStyle() {
  return {
    background: "var(--color-bg)",
    color: "var(--color-text)",
    border: "1px solid var(--color-border)",
    "border-radius": "4px",
    padding: "6px 10px",
    "font-size": "13px",
    "font-family": "inherit",
  };
}

function btnStyle(primary: boolean) {
  return {
    background: primary ? "var(--color-accent)" : "transparent",
    color: primary ? "white" : "var(--color-text)",
    border: `1px solid ${primary ? "var(--color-accent)" : "var(--color-border)"}`,
    "border-radius": "4px",
    padding: "6px 14px",
    "font-size": "13px",
    "font-family": "inherit",
    cursor: "pointer",
  };
}
