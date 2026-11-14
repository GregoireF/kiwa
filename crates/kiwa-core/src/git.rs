//! Minimal git introspection by reading files directly (no git binary needed, ~µs).

use std::fs;
use std::path::{Path, PathBuf};

/// The repository's git directory: `.git`, or where a `.git` file (`gitdir: …`) points for
/// worktrees and submodules.
fn git_dir(dir: &Path) -> Option<PathBuf> {
    let dotgit = dir.join(".git");
    if dotgit.is_dir() {
        return Some(dotgit);
    }
    let text = fs::read_to_string(&dotgit).ok()?;
    let p = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
    Some(if p.is_absolute() { p } else { dir.join(p) })
}

/// Current branch of a repository root, or the short SHA when HEAD is detached.
/// Supports worktrees and submodules (`.git` file containing `gitdir:`).
pub fn branch(dir: &Path) -> Option<String> {
    let head = fs::read_to_string(git_dir(dir)?.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: refs/heads/") {
        Some(b) => Some(b.to_owned()),
        None => Some(head.chars().take(7).collect()),
    }
}

/// The remote's default branch, from `refs/remotes/origin/HEAD` (written by `git clone` and
/// `git remote set-head`). A worktree reads it from the main repository (`commondir`).
pub fn default_branch(dir: &Path) -> Option<String> {
    let mut gd = git_dir(dir)?;
    if let Ok(common) = fs::read_to_string(gd.join("commondir")) {
        let c = PathBuf::from(common.trim());
        gd = if c.is_absolute() { c } else { gd.join(c) };
    }
    let text = fs::read_to_string(gd.join("refs/remotes/origin/HEAD")).ok()?;
    text.trim()
        .strip_prefix("ref: refs/remotes/origin/")
        .filter(|b| !b.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_from_files() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        assert_eq!(branch(r), None);
        fs::create_dir_all(r.join(".git/refs/remotes/origin")).unwrap();
        fs::write(r.join(".git/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        assert_eq!(branch(r).as_deref(), Some("feature/x"));
        assert_eq!(default_branch(r), None, "no origin/HEAD");
        fs::write(
            r.join(".git/refs/remotes/origin/HEAD"),
            "ref: refs/remotes/origin/trunk\n",
        )
        .unwrap();
        assert_eq!(default_branch(r).as_deref(), Some("trunk"));

        // A worktree: `.git` file → worktrees/wt, whose `commondir` leads back to `.git`.
        let wt = r.join("wt");
        let meta = r.join(".git/worktrees/wt");
        fs::create_dir_all(&wt).unwrap();
        fs::create_dir_all(&meta).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", meta.display())).unwrap();
        fs::write(meta.join("HEAD"), "0123456789abcdef\n").unwrap();
        fs::write(meta.join("commondir"), "../..\n").unwrap();
        assert_eq!(
            branch(&wt).as_deref(),
            Some("0123456"),
            "detached: short SHA"
        );
        assert_eq!(default_branch(&wt).as_deref(), Some("trunk"));
    }
}
