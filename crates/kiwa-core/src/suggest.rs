// SPDX-License-Identifier: MIT OR Apache-2.0

//! Candidates for `kw init`, found rather than guessed: kiwa looks where a person is likely
//! to keep code and proposes the folders where projects actually cluster, each with how many
//! projects and tasks it holds, busiest first.
//!
//! Sources, in order:
//! 1. around the current folder: the nearest of it and its parents that holds two projects
//!    or more as direct children;
//! 2. the home folder, four levels deep (enough for `source/repos/<project>`), skipping
//!    hidden folders and the ones operating systems keep application data in;
//! 3. on Windows, every other drive that answers quickly, two levels deep (`D:\work\<project>`).
//!
//! The search only looks for project markers and stops at the first one, so it never walks
//! into a project; it also stops when its time budget is spent. A drive that does not answer
//! in time (an offline network share) is skipped. The first run never stalls.

use crate::{DiscoverOptions, discover};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A folder holding one of these is a project.
const MARKERS: &[&str] = &[
    ".git",
    "package.json",
    "Cargo.toml",
    "go.mod",
    "pyproject.toml",
    "deno.json",
    "composer.json",
    "Gemfile",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "justfile",
    "Taskfile.yml",
    "Makefile",
];
/// Application data and dependency folders: never where people keep their projects.
const NOT_CODE: &[&str] = &[
    "AppData",
    "Library",
    "Applications",
    "node_modules",
    "vendor",
];
const HOME_DEPTH: usize = 4;
const DRIVE_DEPTH: usize = 2;
const DRIVE_ANSWER: Duration = Duration::from_millis(250);
const BUDGET: Duration = Duration::from_millis(1500);
const MAX_CANDIDATES: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub path: PathBuf,
    pub reason: &'static str,
    pub projects: usize,
    pub tasks: usize,
}

fn key(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/');
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.to_owned()
    }
}

fn is_project(dir: &Path) -> bool {
    MARKERS.iter().any(|m| dir.join(m).exists())
        || std::fs::read_dir(dir).is_ok_and(|rd| {
            rd.flatten().any(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                n.ends_with(".sln") || n.ends_with(".csproj")
            })
        })
}

/// Project folders below `root` (not `root` itself), breadth first, never entering a project.
fn projects_below(root: &Path, depth: usize, until: Instant) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = VecDeque::from([(root.to_path_buf(), 0)]);
    while let Some((dir, level)) = queue.pop_front() {
        if level >= depth || Instant::now() > until {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || NOT_CODE.contains(&name.as_str()) {
                continue;
            }
            let path = e.path();
            if !path.is_dir() {
                continue;
            }
            if is_project(&path) {
                found.push(path);
            } else {
                queue.push_back((path, level + 1));
            }
        }
    }
    found
}

/// Folders that directly contain projects, with how many, most first.
fn clusters(projects: &[PathBuf]) -> Vec<(PathBuf, usize)> {
    let mut count: HashMap<&Path, usize> = HashMap::new();
    for parent in projects.iter().filter_map(|p| p.parent()) {
        *count.entry(parent).or_default() += 1;
    }
    let mut v: Vec<_> = count
        .into_iter()
        .map(|(p, n)| (p.to_path_buf(), n))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// Whether `path` answers within `limit` (a disconnected network drive may block for long).
fn answers(path: PathBuf, limit: Duration) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(path.is_dir()));
    rx.recv_timeout(limit).unwrap_or(false)
}

/// Drive roots other than the home drive, on Windows; none elsewhere.
fn other_drives(home: Option<&Path>) -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let home_drive = home.and_then(|h| h.components().next());
    ('C'..='Z')
        .map(|c| PathBuf::from(format!("{c}:\\")))
        .filter(|d| d.components().next() != home_drive)
        .collect()
}

/// Candidate folders and why each is proposed, without duplicates.
pub fn candidates(cwd: &Path, home: Option<&Path>) -> Vec<(PathBuf, &'static str)> {
    candidates_in(cwd, home, &other_drives(home))
}

fn candidates_in(
    cwd: &Path,
    home: Option<&Path>,
    drives: &[PathBuf],
) -> Vec<(PathBuf, &'static str)> {
    let until = Instant::now() + BUDGET;
    let mut out: Vec<(PathBuf, &'static str)> = Vec::new();
    if let Some(dir) = cwd
        .ancestors()
        .take(4)
        .find(|d| projects_below(d, 1, until).len() >= 2)
    {
        out.push((dir.to_path_buf(), "around the current folder"));
    }
    if let Some(home) = home {
        let found = clusters(&projects_below(home, HOME_DEPTH, until));
        out.extend(
            found
                .into_iter()
                .map(|(p, _)| (p, "under your home folder")),
        );
    }
    let found: Vec<(PathBuf, usize)> = drives
        .par_iter()
        .filter(|d| answers((*d).clone(), DRIVE_ANSWER))
        .flat_map(|d| clusters(&projects_below(d, DRIVE_DEPTH, until)))
        .collect();
    out.extend(found.into_iter().map(|(p, _)| (p, "on another drive")));
    let mut seen = HashSet::new();
    out.retain(|(p, _)| seen.insert(key(p)));
    out.truncate(MAX_CANDIDATES);
    out
}

/// Candidates with their project and task counts (three levels deep), busiest first.
pub fn suggest(cwd: &Path, home: Option<&Path>) -> Vec<Suggestion> {
    counted(candidates(cwd, home))
}

fn counted(candidates: Vec<(PathBuf, &'static str)>) -> Vec<Suggestion> {
    let mut s: Vec<Suggestion> = candidates
        .into_par_iter()
        .map(|(path, reason)| {
            let projects = discover(&DiscoverOptions {
                roots: vec![path.clone()],
                max_depth: 3,
                ignore: Vec::new(),
            });
            Suggestion {
                tasks: projects.iter().map(|p| p.tasks.len()).sum(),
                projects: projects.len(),
                path,
                reason,
            }
        })
        .collect();
    s.sort_by_key(|x| std::cmp::Reverse(x.projects));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn project(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("go.mod"), "module x").unwrap();
    }

    #[test]
    fn proposes_where_projects_cluster_under_home() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        project(&h.join("source/repos/a"));
        project(&h.join("source/repos/b"));
        project(&h.join("source/repos/b/tools"));
        project(&h.join("work/c"));
        fs::create_dir_all(h.join("music/albums")).unwrap();
        project(&h.join("AppData/Local/thing"));
        project(&h.join(".cache/thing"));
        let cwd = tempfile::tempdir().unwrap();
        let s = counted(candidates_in(cwd.path(), Some(h), &[]));
        let keys: Vec<String> = s.iter().map(|x| key(&x.path)).collect();
        assert_eq!(
            keys,
            [key(&h.join("source/repos")), key(&h.join("work"))],
            "busiest first; no empty, hidden or app-data folder; a nested package is no cluster"
        );
        assert_eq!(s[0].reason, "under your home folder");
        assert_eq!(s[0].projects, 3, "the count includes nested packages");
    }

    #[test]
    fn the_folder_around_you_comes_first() {
        let ws = tempfile::tempdir().unwrap();
        project(&ws.path().join("app"));
        project(&ws.path().join("api"));
        let inside = ws.path().join("app/src");
        fs::create_dir_all(&inside).unwrap();
        let c = candidates_in(&inside, None, &[]);
        assert_eq!(key(&c[0].0), key(ws.path()));
        assert_eq!(c[0].1, "around the current folder");
        let lonely = tempfile::tempdir().unwrap();
        project(&lonely.path().join("only"));
        assert!(
            candidates_in(&lonely.path().join("only"), None, &[]).is_empty(),
            "one project is not a cluster"
        );
    }

    #[test]
    fn drives_are_searched_shallowly() {
        let drive = tempfile::tempdir().unwrap();
        project(&drive.path().join("work/a"));
        project(&drive.path().join("deep/er/b"));
        let cwd = tempfile::tempdir().unwrap();
        let c = candidates_in(cwd.path(), None, &[drive.path().to_path_buf()]);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(
            (key(&c[0].0), c[0].1),
            (key(&drive.path().join("work")), "on another drive")
        );
    }
}
