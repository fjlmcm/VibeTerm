//! 集成测试 — 用临时 git repo 验证整链路
//!
//! 跳过条件:CI 环境没 git 时 list_worktrees 等会返回 GitNotFound,测试统一 short-circuit

use std::path::Path;
use std::process::Command as StdCommand;

use tempfile::TempDir;
use vibeterm_git::{
    add_worktree, is_git_repo, list_local_branches, list_worktrees, remove_worktree,
    worktree_status, BranchSpec,
};

fn git_available() -> bool {
    StdCommand::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn init_repo(dir: &Path) {
    run(dir, &["init", "-q", "-b", "main"]);
    run(dir, &["config", "user.email", "test@vibeterm.local"]);
    run(dir, &["config", "user.name", "VibeTerm Test"]);
    std::fs::write(dir.join("README.md"), b"# test\n").unwrap();
    run(dir, &["add", "."]);
    run(dir, &["commit", "-q", "-m", "init"]);
}

fn run(cwd: &Path, args: &[&str]) {
    let out = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test]
async fn is_git_repo_detects_inside_and_outside() {
    if !git_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("r");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);

    assert!(is_git_repo(&repo).await.unwrap());

    let non_repo = tmp.path().join("plain");
    std::fs::create_dir(&non_repo).unwrap();
    assert!(!is_git_repo(&non_repo).await.unwrap());
}

#[tokio::test]
async fn full_worktree_lifecycle() {
    if !git_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);

    // 初始只有一个 worktree(主)
    let list0 = list_worktrees(&repo).await.unwrap();
    assert_eq!(list0.len(), 1);
    assert!(list0[0].branch.as_deref().unwrap_or("").ends_with("main"));

    // 在 repo 外面建一个 worktree(避免 nested)
    let wt = tmp.path().join("wt-feat");
    let entry = add_worktree(&repo, &wt, BranchSpec::NewFromHead("feat-x".into()))
        .await
        .unwrap();
    assert!(entry.branch.as_deref().unwrap_or("").ends_with("feat-x"));

    let list1 = list_worktrees(&repo).await.unwrap();
    assert_eq!(list1.len(), 2);

    // 状态 — clean
    let st = worktree_status(&wt).await.unwrap();
    assert_eq!(st.branch.as_deref(), Some("feat-x"));
    assert!(!st.is_dirty);

    // 改文件 → dirty
    std::fs::write(wt.join("README.md"), b"# dirty\n").unwrap();
    let st2 = worktree_status(&wt).await.unwrap();
    assert!(st2.is_dirty);

    // 列分支应能看到 main + feat-x
    let branches = list_local_branches(&repo).await.unwrap();
    assert!(branches.iter().any(|b| b == "main"));
    assert!(branches.iter().any(|b| b == "feat-x"));

    // 清掉 dirty 才能 remove(不 force)
    std::fs::write(wt.join("README.md"), b"# test\n").unwrap();
    remove_worktree(&repo, &wt, false).await.unwrap();
    let list2 = list_worktrees(&repo).await.unwrap();
    assert_eq!(list2.len(), 1);
}

#[tokio::test]
async fn remove_force_when_dirty() {
    if !git_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);

    let wt = tmp.path().join("wt-dirty");
    add_worktree(&repo, &wt, BranchSpec::NewFromHead("dirty-branch".into()))
        .await
        .unwrap();
    std::fs::write(wt.join("new.txt"), b"new\n").unwrap();

    // 非 force 应该失败
    let err = remove_worktree(&repo, &wt, false).await;
    assert!(err.is_err());

    // force 能成功
    remove_worktree(&repo, &wt, true).await.unwrap();
    assert_eq!(list_worktrees(&repo).await.unwrap().len(), 1);
}
