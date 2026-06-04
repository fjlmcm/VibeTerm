//! vibeterm-git — `git worktree` 薄封装(L1)
//!
//! 本 crate 只做四件事:
//!   1. `is_git_repo(path)`         — 判断路径是否在 git 工作树内
//!   2. `list_worktrees(repo_path)` — `git worktree list --porcelain`
//!   3. `add_worktree(...)`         — `git worktree add`
//!   4. `remove_worktree(...)`      — `git worktree remove`
//!   + `worktree_status(path)`      — branch / head / dirty / ahead / behind
//!
//! 设计:用 `tokio::process::Command` 调 `git` CLI(不引 gix/git2),
//!       理由:零编译依赖、行为跟用户终端 100% 一致、L1 阶段只读 porcelain 完全够。
//!
//! 错误统一为 `GitError`,所有路径用 `&Path`。

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("git binary not found in PATH")]
    GitNotFound,
    #[error("not a git repository: {0}")]
    NotARepo(PathBuf),
    #[error("git command failed: {0}")]
    CommandFailed(String),
    #[error("invalid argument: {0}")]
    InvalidArg(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
}

/// 一条 worktree 记录(`git worktree list --porcelain` 解析结果)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    pub head: String,
    /// 如 `refs/heads/feature-x`;detached HEAD 时为 None
    pub branch: Option<String>,
    pub is_bare: bool,
    pub is_detached: bool,
    pub is_locked: bool,
}

/// 工作树状态(由 `git status --porcelain=v2 --branch` 解析)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorktreeStatus {
    pub branch: Option<String>,
    pub head: String,
    pub is_dirty: bool,
    pub ahead: u32,
    pub behind: u32,
    /// 已 stage 的文件数 (index 中变更)
    #[serde(default)]
    pub staged: u32,
    /// 已修改未 stage 的文件数 (worktree 变更)
    #[serde(default)]
    pub unstaged: u32,
    /// 未跟踪文件数
    #[serde(default)]
    pub untracked: u32,
}

/// 新建 worktree 时的分支策略
#[derive(Debug, Clone)]
pub enum BranchSpec {
    /// 用一个已存在的分支
    Existing(String),
    /// 新建分支(从当前 HEAD)
    NewFromHead(String),
    /// 新建分支(从指定 ref)
    NewFromRef { name: String, start_point: String },
}

/// 判断 `path` 是否在 git 工作树内
pub async fn is_git_repo(path: &Path) -> Result<bool, GitError> {
    let out = run_git(path, &["rev-parse", "--is-inside-work-tree"]).await;
    match out {
        Ok(s) => Ok(s.trim() == "true"),
        Err(GitError::CommandFailed(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// 解析 `path` 所属的主仓库 .git 公共目录(`git rev-parse --git-common-dir` 的 parent)
pub async fn repo_common_root(path: &Path) -> Result<PathBuf, GitError> {
    let out = run_git(path, &["rev-parse", "--show-toplevel"]).await?;
    Ok(PathBuf::from(out.trim()))
}

/// `git worktree list --porcelain` 解析
pub async fn list_worktrees(repo_path: &Path) -> Result<Vec<WorktreeEntry>, GitError> {
    let out = run_git(repo_path, &["worktree", "list", "--porcelain"]).await?;
    parse_worktree_porcelain(&out)
}

fn parse_worktree_porcelain(text: &str) -> Result<Vec<WorktreeEntry>, GitError> {
    let mut entries = Vec::new();
    let mut cur: Option<WorktreeEntry> = None;
    for line in text.lines() {
        if line.is_empty() {
            if let Some(e) = cur.take() {
                entries.push(e);
            }
            continue;
        }
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => {
                if let Some(e) = cur.take() {
                    entries.push(e);
                }
                cur = Some(WorktreeEntry {
                    path: rest.to_string(),
                    head: String::new(),
                    branch: None,
                    is_bare: false,
                    is_detached: false,
                    is_locked: false,
                });
            }
            "HEAD" => {
                if let Some(e) = cur.as_mut() {
                    e.head = rest.to_string();
                }
            }
            "branch" => {
                if let Some(e) = cur.as_mut() {
                    e.branch = Some(rest.to_string());
                }
            }
            "bare" => {
                if let Some(e) = cur.as_mut() {
                    e.is_bare = true;
                }
            }
            "detached" => {
                if let Some(e) = cur.as_mut() {
                    e.is_detached = true;
                }
            }
            "locked" => {
                if let Some(e) = cur.as_mut() {
                    e.is_locked = true;
                }
            }
            _ => {}
        }
    }
    if let Some(e) = cur.take() {
        entries.push(e);
    }
    Ok(entries)
}

/// 拒绝以 `-` 开头的名称(branch / start_point),否则 git 会把它当作选项解析,
/// 形成参数级注入(如 `--upload-pack=...`)。`field` 用于错误信息。
fn reject_dash_prefix(field: &str, value: &str) -> Result<(), GitError> {
    if value.starts_with('-') {
        return Err(GitError::InvalidArg(format!(
            "{field} must not start with '-': {value:?}"
        )));
    }
    Ok(())
}

/// `git worktree add <new_path> <branch_spec>`
///
/// 返回新建 worktree 的 `WorktreeEntry`(通过随后 list 找到 path 匹配项)
pub async fn add_worktree(
    repo_path: &Path,
    new_path: &Path,
    spec: BranchSpec,
) -> Result<WorktreeEntry, GitError> {
    let new_path_s = new_path.to_string_lossy().to_string();
    // 拒绝以 '-' 开头的 branch/start_point,避免被 git 解析为选项(参数注入纵深防御)
    let mut args: Vec<String> = vec!["worktree".into(), "add".into()];
    match &spec {
        BranchSpec::Existing(branch) => {
            reject_dash_prefix("branch", branch)?;
            // "--" 之后的参数 git 一律视为操作数(path / commit-ish),不再当选项
            args.push("--".into());
            args.push(new_path_s.clone());
            args.push(branch.clone());
        }
        BranchSpec::NewFromHead(name) => {
            reject_dash_prefix("branch", name)?;
            args.push("-b".into());
            args.push(name.clone());
            args.push("--".into());
            args.push(new_path_s.clone());
        }
        BranchSpec::NewFromRef { name, start_point } => {
            reject_dash_prefix("branch", name)?;
            reject_dash_prefix("start_point", start_point)?;
            args.push("-b".into());
            args.push(name.clone());
            args.push("--".into());
            args.push(new_path_s.clone());
            args.push(start_point.clone());
        }
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_git(repo_path, &arg_refs).await?;

    // 找回这条新 worktree:用 canonicalize 匹配,处理 git 输出可能用绝对路径
    let canon_new = std::fs::canonicalize(new_path).unwrap_or_else(|_| new_path.to_path_buf());
    let list = list_worktrees(repo_path).await?;
    list.into_iter()
        .find(|e| {
            let p = std::fs::canonicalize(&e.path).unwrap_or_else(|_| PathBuf::from(&e.path));
            p == canon_new
        })
        .ok_or_else(|| GitError::Parse(format!("new worktree not found after add: {:?}", new_path)))
}

/// `git worktree remove [--force] <path>`
pub async fn remove_worktree(
    repo_path: &Path,
    worktree_path: &Path,
    force: bool,
) -> Result<(), GitError> {
    let p = worktree_path.to_string_lossy().to_string();
    // "--" 把路径与选项隔开,避免以 '-' 开头的路径被 git 当作选项
    let args: Vec<&str> = if force {
        vec!["worktree", "remove", "--force", "--", &p]
    } else {
        vec!["worktree", "remove", "--", &p]
    };
    run_git(repo_path, &args).await?;
    Ok(())
}

/// 当前工作树状态(branch / head / dirty / ahead / behind)
pub async fn worktree_status(worktree_path: &Path) -> Result<WorktreeStatus, GitError> {
    // porcelain=v2 + --branch:第一行是 `# branch.head ...`,后续是 `# branch.ab +N -M`,然后是变更项
    let out = run_git(worktree_path, &["status", "--porcelain=v2", "--branch"]).await?;
    parse_status_porcelain_v2(&out)
}

fn parse_status_porcelain_v2(text: &str) -> Result<WorktreeStatus, GitError> {
    let mut st = WorktreeStatus::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            if rest != "(detached)" {
                st.branch = Some(rest.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("# branch.oid ") {
            st.head = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // 形如 "+1 -2"
            let mut it = rest.split_whitespace();
            if let (Some(a), Some(b)) = (it.next(), it.next()) {
                st.ahead = a.trim_start_matches('+').parse().unwrap_or(0);
                st.behind = b.trim_start_matches('-').parse().unwrap_or(0);
            }
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            // 任何非 header 非空行 = 有变更
            st.is_dirty = true;
            // porcelain v2 行格式:
            //   "1 XY ..." — ordinary tracked file (index status X, worktree status Y)
            //   "2 XY ..." — renamed/copied
            //   "u XY ..." — unmerged
            //   "? ..."    — untracked
            //   "! ..."    — ignored (默认不显)
            let bytes = line.as_bytes();
            match bytes.first() {
                Some(b'?') => st.untracked = st.untracked.saturating_add(1),
                Some(b'1') | Some(b'2') | Some(b'u') => {
                    // bytes[2..4] = "XY" — X 是 index, Y 是 worktree. 任一非 '.' / 空 即对应区有改动
                    if let (Some(&x), Some(&y)) = (bytes.get(2), bytes.get(3)) {
                        if x != b'.' && x != b' ' {
                            st.staged = st.staged.saturating_add(1);
                        }
                        if y != b'.' && y != b' ' {
                            st.unstaged = st.unstaged.saturating_add(1);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(st)
}

/// stash 数量 (`git stash list | wc -l`). 没 stash 返回 0; 命令失败返回 Err.
pub async fn stash_count(repo_path: &Path) -> Result<u32, GitError> {
    let out = run_git(repo_path, &["stash", "list"]).await?;
    Ok(out.lines().filter(|l| !l.trim().is_empty()).count() as u32)
}

/// 列出仓库所有本地分支(refs/heads/<name>),供 UI autocomplete
pub async fn list_local_branches(repo_path: &Path) -> Result<Vec<String>, GitError> {
    let out = run_git(
        repo_path,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads/"],
    )
    .await?;
    Ok(out
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// 极速读当前分支名:直接读 `.git/HEAD`(含 worktree gitdir 文件解析),**不 spawn git**。
/// detached / 读失败 → None。用于热路径(侧栏分支显示),比 spawn `git status` 轻量得多。
/// 借鉴 cmux `TabManager.gitBranchName`(直读 .git/HEAD 而非 spawn)。
pub fn branch_fast(worktree_path: &Path) -> Option<String> {
    let dot_git = worktree_path.join(".git");
    let head_path = match std::fs::metadata(&dot_git) {
        Ok(m) if m.is_dir() => dot_git.join("HEAD"),
        Ok(_) => {
            // worktree:.git 是文件,内容 "gitdir: <abs path to .git/worktrees/<name>>"
            let content = std::fs::read_to_string(&dot_git).ok()?;
            let gitdir = content.lines().next()?.strip_prefix("gitdir:")?.trim();
            Path::new(gitdir).join("HEAD")
        }
        Err(_) => return None,
    };
    let head = std::fs::read_to_string(&head_path).ok()?;
    // "ref: refs/heads/<name>" → name;否则 detached(裸 sha)→ None
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_string)
}

/// diff 的三个来源(对照 cmux diff-viewer 的 unstaged/staged/branch;**不含** agent-turn,那需 hook)。
#[derive(Debug, Clone)]
pub enum DiffSource {
    /// 工作树 vs 暂存区(`git diff`)
    Unstaged,
    /// 暂存区 vs HEAD(`git diff --cached`)
    Staged,
    /// 工作树 vs 某个基准 ref(`git diff <ref>`)—— 看相对 base 分支的全部分歧(含未提交)
    VsRef(String),
}

/// 生成 unified diff 文本(`--no-color`,前端自行解析渲染)。纯只读,零侵入。
pub async fn diff(worktree_path: &Path, source: &DiffSource) -> Result<String, GitError> {
    let mut args: Vec<String> = vec!["diff".into(), "--no-color".into()];
    match source {
        DiffSource::Unstaged => {}
        DiffSource::Staged => args.push("--cached".into()),
        DiffSource::VsRef(r) => {
            reject_dash_prefix("ref", r)?;
            args.push(r.clone());
        }
    }
    // "--" 隔开,避免任何后续被当选项
    args.push("--".into());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_git(worktree_path, &arg_refs).await
}

/// 推断默认基准分支:依次试 `origin/HEAD` 指向、`main`、`master`、`develop`。找不到返回 None。
/// 给 VsRef diff 当默认 base。
pub async fn default_base_ref(worktree_path: &Path) -> Option<String> {
    // origin/HEAD → 形如 "refs/remotes/origin/main"
    if let Ok(s) = run_git(
        worktree_path,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )
    .await
    {
        if let Some(name) = s.trim().strip_prefix("refs/remotes/origin/") {
            if !name.is_empty() {
                return Some(format!("origin/{name}"));
            }
        }
    }
    for cand in ["main", "master", "develop"] {
        if run_git(worktree_path, &["rev-parse", "--verify", "--quiet", cand])
            .await
            .is_ok()
        {
            return Some(cand.to_string());
        }
    }
    None
}

// ---- 内部 ----

async fn run_git(cwd: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::GitNotFound
            } else {
                GitError::Io(e)
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(GitError::CommandFailed(format!(
            "git {} (cwd={:?}): {}",
            args.join(" "),
            cwd,
            stderr
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn worktree_porcelain_basic() {
        let s = "\
worktree /repo/main
HEAD abc123
branch refs/heads/main

worktree /repo/.worktrees/feat
HEAD def456
branch refs/heads/feat

worktree /repo/.worktrees/detached
HEAD 0123abc
detached
";
        let r = parse_worktree_porcelain(s).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].path, "/repo/main");
        assert_eq!(r[0].branch.as_deref(), Some("refs/heads/main"));
        assert_eq!(r[1].path, "/repo/.worktrees/feat");
        assert!(r[2].is_detached);
        assert_eq!(r[2].branch, None);
    }

    #[test]
    fn worktree_porcelain_with_locked() {
        let s = "\
worktree /a
HEAD abc
branch refs/heads/x
locked

worktree /b
HEAD def
bare
";
        let r = parse_worktree_porcelain(s).unwrap();
        assert_eq!(r.len(), 2);
        assert!(r[0].is_locked);
        assert!(r[1].is_bare);
    }

    #[test]
    fn status_porcelain_clean() {
        let s = "\
# branch.oid abc123def
# branch.head main
# branch.upstream origin/main
# branch.ab +0 -0
";
        let st = parse_status_porcelain_v2(s).unwrap();
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.head, "abc123def");
        assert_eq!(st.ahead, 0);
        assert_eq!(st.behind, 0);
        assert!(!st.is_dirty);
    }

    #[test]
    fn status_porcelain_dirty_ahead_behind() {
        let s = "\
# branch.oid abc123def
# branch.head feat-x
# branch.upstream origin/feat-x
# branch.ab +2 -1
1 .M N... 100644 100644 100644 abc abc src/foo.rs
? unknown.txt
";
        let st = parse_status_porcelain_v2(s).unwrap();
        assert_eq!(st.branch.as_deref(), Some("feat-x"));
        assert_eq!(st.ahead, 2);
        assert_eq!(st.behind, 1);
        assert!(st.is_dirty);
    }

    #[test]
    fn status_porcelain_detached() {
        let s = "\
# branch.oid abc123def
# branch.head (detached)
";
        let st = parse_status_porcelain_v2(s).unwrap();
        assert_eq!(st.branch, None);
        assert_eq!(st.head, "abc123def");
        assert!(!st.is_dirty);
    }
}

#[cfg(test)]
mod branch_fast_tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_branch_from_plain_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let git = tmp.path().join(".git");
        fs::create_dir(&git).unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/feature-x\n").unwrap();
        assert_eq!(branch_fast(tmp.path()).as_deref(), Some("feature-x"));
    }

    #[test]
    fn detached_head_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let git = tmp.path().join(".git");
        fs::create_dir(&git).unwrap();
        fs::write(git.join("HEAD"), "0123456789abcdef0123456789abcdef01234567\n").unwrap();
        assert_eq!(branch_fast(tmp.path()), None);
    }

    #[test]
    fn resolves_worktree_gitdir_file() {
        // worktree:工作树根的 .git 是文件 "gitdir: <主仓 .git/worktrees/<name>>"
        let tmp = tempfile::tempdir().unwrap();
        let real_gitdir = tmp.path().join("main/.git/worktrees/feat");
        fs::create_dir_all(&real_gitdir).unwrap();
        fs::write(real_gitdir.join("HEAD"), "ref: refs/heads/feat\n").unwrap();
        let wt = tmp.path().join("wt-feat");
        fs::create_dir(&wt).unwrap();
        fs::write(
            wt.join(".git"),
            format!("gitdir: {}\n", real_gitdir.display()),
        )
        .unwrap();
        assert_eq!(branch_fast(&wt).as_deref(), Some("feat"));
    }

    #[test]
    fn missing_git_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(branch_fast(tmp.path()), None);
    }
}
