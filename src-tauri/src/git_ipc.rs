//! Git Worktree / 状态 IPC:git_* 命令 + worktree 状态合成 + 路径安全门(safe_cwd)。
//! 从 main.rs 拆出(行为不变)。

use vibeterm_ipc::{IpcError, IpcResult};

use crate::now_ms;

// ============================
// IPC commands — Git Worktree
// ============================
//
// 命名一律 git_* 前缀,前端 ipc/index.ts 一一对应。
// 错误统一映射 IpcError::Unknown { trace_id: "git:<detail>" }。

pub(crate) fn map_git_err(e: vibeterm_git::GitError) -> IpcError {
    IpcError::Unknown {
        trace_id: format!("git:{e}"),
    }
}

/// 把 vibeterm-git 解析出来的 entry + 实时 status 合成 IPC 层 WorktreeRef。
/// `branch` 字段:用 status.branch(短名)优于 entry.branch(refs/heads/ 前缀)。
pub(crate) async fn build_worktree_ref(
    repo_path: &std::path::Path,
    worktree_path: &std::path::Path,
) -> Result<vibeterm_ipc::WorktreeRef, vibeterm_git::GitError> {
    let st = vibeterm_git::worktree_status(worktree_path).await?;
    Ok(vibeterm_ipc::WorktreeRef {
        repo_path: repo_path.to_string_lossy().into_owned(),
        worktree_path: worktree_path.to_string_lossy().into_owned(),
        branch: st.branch.clone(),
        head: st.head,
        is_dirty: st.is_dirty,
        ahead: st.ahead,
        behind: st.behind,
        status_updated_at: now_ms(),
    })
}

// git 命令的 repo/cwd 路径统一走 safe_cwd(canonicalize + is_dir),与 git_status_brief
// 等保持一致;new_path(尚不存在)/worktree_path(git 只认已注册 worktree)交给 git 自验。
pub(crate) fn git_repo_dir(repo_path: &str) -> IpcResult<std::path::PathBuf> {
    safe_cwd(repo_path).ok_or_else(|| IpcError::NotFound {
        resource: "directory".into(),
        id: repo_path.to_string(),
    })
}

#[tauri::command]
pub(crate) async fn git_is_repo(path: String) -> IpcResult<bool> {
    let Some(dir) = safe_cwd(&path) else {
        return Ok(false); // 路径无效 = 不是仓库,不报错(task cwd 可能已被删除)
    };
    vibeterm_git::is_git_repo(&dir).await.map_err(map_git_err)
}

#[tauri::command]
pub(crate) async fn git_repo_root(path: String) -> IpcResult<String> {
    vibeterm_git::repo_common_root(&git_repo_dir(&path)?)
        .await
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(map_git_err)
}

#[tauri::command]
pub(crate) async fn git_list_branches(repo_path: String) -> IpcResult<Vec<String>> {
    vibeterm_git::list_local_branches(&git_repo_dir(&repo_path)?)
        .await
        .map_err(map_git_err)
}

#[tauri::command]
pub(crate) async fn git_add_worktree(
    repo_path: String,
    new_path: String,
    spec: vibeterm_ipc::BranchSpecDto,
) -> IpcResult<vibeterm_ipc::WorktreeRef> {
    let repo = git_repo_dir(&repo_path)?;
    let repo = repo.as_path();
    let new = std::path::Path::new(&new_path);
    let bs = match spec {
        vibeterm_ipc::BranchSpecDto::Existing { branch } => {
            vibeterm_git::BranchSpec::Existing(branch)
        }
        vibeterm_ipc::BranchSpecDto::NewFromHead { branch } => {
            vibeterm_git::BranchSpec::NewFromHead(branch)
        }
        vibeterm_ipc::BranchSpecDto::NewFromRef {
            branch,
            start_point,
        } => vibeterm_git::BranchSpec::NewFromRef {
            name: branch,
            start_point,
        },
    };
    let _entry = vibeterm_git::add_worktree(repo, new, bs)
        .await
        .map_err(map_git_err)?;
    build_worktree_ref(repo, new).await.map_err(map_git_err)
}

#[tauri::command]
pub(crate) async fn git_remove_worktree(
    repo_path: String,
    worktree_path: String,
    force: bool,
) -> IpcResult<()> {
    vibeterm_git::remove_worktree(
        &git_repo_dir(&repo_path)?,
        std::path::Path::new(&worktree_path),
        force,
    )
    .await
    .map_err(map_git_err)
}

/// 把持久化的 worktree 路径(来自 tasks.json 反序列化)规范化后返回。
/// 防御性:canonicalize 消除 `../` 等,确保 git status 不在非预期目录执行;
/// 失败(路径不存在/非法)则返回 None,调用方跳过该条目并 warn。
pub(crate) fn validated_worktree_path(raw: &str) -> Option<std::path::PathBuf> {
    match std::fs::canonicalize(raw) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!(path = raw, err = %e, "worktree path canonicalize failed, skipping");
            None
        }
    }
}

// ============================
// IPC commands — Theme / Config
// ============================

/// 把前端传入的 cwd 字符串规范化为绝对 + canonicalize 后的目录路径.
/// 用于所有"按 cwd 拉某种状态"的 IPC: 防 symlink TOCTOU 跳到敏感目录,
/// 同时保证传给 Command::current_dir 的路径是稳定的真实路径.
/// 失败 (路径不存在 / 不是目录 / canonicalize 报错) 返回 None.
pub(crate) fn safe_cwd(cwd: &str) -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(cwd);
    let canon = std::fs::canonicalize(&p).ok()?;
    if !canon.is_dir() {
        return None;
    }
    Some(canon)
}

/// 拿 cwd 对应的 git 简略状态 (branch / dirty / ahead / behind / staged / unstaged / untracked).
/// 非 git 仓库或路径无效 → None.
#[tauri::command]
pub(crate) async fn git_status_brief(
    cwd: String,
) -> IpcResult<Option<vibeterm_git::WorktreeStatus>> {
    let Some(path) = safe_cwd(&cwd) else {
        return Ok(None);
    };
    Ok(vibeterm_git::worktree_status(&path).await.ok())
}

/// stash 数量. 没 stash 或非 git 仓库返回 0.
#[tauri::command]
pub(crate) async fn git_stash_count(cwd: String) -> IpcResult<u32> {
    let Some(path) = safe_cwd(&cwd) else {
        return Ok(0);
    };
    Ok(vibeterm_git::stash_count(&path).await.unwrap_or(0))
}

/// 三源 diff 结果(借鉴 cmux diff-viewer 的 unstaged/staged/branch;**不含** agent-turn,那需 hook).
#[derive(serde::Serialize, specta::Type)]
pub(crate) struct GitDiffResult {
    source: String,
    /// VsRef 实际用的 base ref(自动推断时回填,供 UI 显示).
    base: Option<String>,
    /// unified diff 原文(--no-color),前端解析渲染.
    raw: String,
    /// 是否因超大被截断(防前端渲染卡死).
    truncated: bool,
}

/// 生成某个 worktree 的 diff. source: "unstaged" / "staged" / "base".
/// "base" 时 base 参数留空则自动推断(origin/HEAD → main → master → develop).
/// 纯只读 `git diff`,零侵入。非 git / 无效路径 / 无法定 base → None.
#[tauri::command]
pub(crate) async fn git_diff(
    cwd: String,
    source: String,
    base: Option<String>,
) -> IpcResult<Option<GitDiffResult>> {
    let Some(path) = safe_cwd(&cwd) else {
        return Ok(None);
    };
    use vibeterm_git::DiffSource;
    let (src, base_used) = match source.as_str() {
        "unstaged" => (DiffSource::Unstaged, None),
        "staged" => (DiffSource::Staged, None),
        "base" => {
            let base_ref = match base {
                Some(b) if !b.trim().is_empty() => b,
                _ => match vibeterm_git::default_base_ref(&path).await {
                    Some(b) => b,
                    None => return Ok(None),
                },
            };
            (DiffSource::VsRef(base_ref.clone()), Some(base_ref))
        }
        _ => return Ok(None),
    };
    let raw = vibeterm_git::diff(&path, &src)
        .await
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("git_diff:{e}"),
        })?;
    // 限额 ~2MB,防超大 diff 卡死前端渲染(借鉴 cmux DiffViewerLimits).
    const MAX: usize = 2_000_000;
    let truncated = raw.len() > MAX;
    let raw = if truncated {
        let mut end = MAX;
        while !raw.is_char_boundary(end) {
            end -= 1;
        }
        raw[..end].to_string()
    } else {
        raw
    };
    Ok(Some(GitDiffResult {
        source,
        base: base_used,
        raw,
        truncated,
    }))
}

/// 当前分支的 PR 状态 (用 gh CLI). 没装 gh / 没仓库 / 没 PR 都返回 None.
/// 返回值: "open" / "draft" / "merged" / "closed" / None.
///
/// **超时 5s**: gh CLI 初次使用会触发 auth 提示, 或网络慢时可挂分钟级,
/// 必须有 hard timeout 防止状态栏 refresh tick 被卡死.
#[tauri::command]
pub(crate) async fn gh_pr_status(cwd: String) -> IpcResult<Option<String>> {
    let Some(path) = safe_cwd(&cwd) else {
        return Ok(None);
    };
    let fut = tokio::process::Command::new("gh")
        .args([
            "pr",
            "view",
            "--json",
            "state,isDraft",
            "-q",
            ".state,.isDraft",
        ])
        .current_dir(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    let out = match tokio::time::timeout(std::time::Duration::from_secs(5), fut).await {
        Ok(r) => r,
        Err(_) => {
            tracing::debug!("gh_pr_status: 5s timeout, treat as no PR");
            return Ok(None);
        }
    };
    let Ok(out) = out else { return Ok(None) };
    if !out.status.success() {
        return Ok(None);
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut lines = s.lines();
    let state = lines.next().unwrap_or("").trim();
    let is_draft = lines.next().unwrap_or("").trim() == "true";
    let label = match state {
        "OPEN" if is_draft => "draft",
        "OPEN" => "open",
        "MERGED" => "merged",
        "CLOSED" => "closed",
        _ => return Ok(None),
    };
    Ok(Some(label.to_string()))
}
