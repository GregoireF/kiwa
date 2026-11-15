// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::detect::detect_dir;
use crate::git;
use crate::model::Project;
use ignore::WalkBuilder;
use rayon::prelude::*;
use std::path::{Path, PathBuf};

/// Directories that hold installed dependencies or caches: never user projects.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "vendor",
    "__pycache__",
    "venv",
    "site-packages",
    "bower_components",
    "Pods",
    "DerivedData",
];

/// Build output names. Skipped only *inside* a project (parent has a manifest), so a
/// workspace folder that happens to be called `build` or `bin` is still scanned.
const OUTPUT_DIRS: &[&str] = &["target", "dist", "build", "out", "bin", "obj"];
const MANIFESTS: &[&str] = &[
    "package.json",
    "Cargo.toml",
    "go.mod",
    "pyproject.toml",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "composer.json",
    "deno.json",
    "Makefile",
];

fn inside_project(dir: &Path) -> bool {
    if MANIFESTS.iter().any(|m| dir.join(m).is_file()) {
        return true;
    }
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten().any(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n.ends_with(".csproj") || n.ends_with(".fsproj") || n.ends_with(".sln")
        })
    })
}

#[derive(Debug, Clone)]
pub struct DiscoverOptions {
    pub roots: Vec<PathBuf>,
    /// Maximum directory depth below each root (root = 0).
    pub max_depth: usize,
    /// Extra directory names to skip.
    pub ignore: Vec<String>,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        Self {
            roots: vec![PathBuf::from(".")],
            max_depth: 4,
            ignore: Vec::new(),
        }
    }
}

/// Walk every root (respecting .gitignore, skipping hidden and build dirs),
/// then run detectors on candidate directories in parallel.
pub fn discover(opts: &DiscoverOptions) -> Vec<Project> {
    let mut candidates: Vec<(PathBuf, PathBuf)> = Vec::new();
    for root in &opts.roots {
        let root = dunce(root);
        let extra = opts.ignore.clone();
        let walker = WalkBuilder::new(&root)
            .max_depth(Some(opts.max_depth))
            // Symlinked and junctioned project folders count; `ignore` detects loops.
            .follow_links(true)
            .filter_entry(move |e| {
                let n = e.file_name().to_string_lossy();
                if SKIP_DIRS.contains(&n.as_ref()) || extra.iter().any(|x| x == n.as_ref()) {
                    return false;
                }
                let output = OUTPUT_DIRS.contains(&n.as_ref())
                    && e.depth() > 0
                    && e.path().parent().is_some_and(inside_project);
                !output
            })
            .build();
        for entry in walker.flatten() {
            if entry.file_type().is_some_and(|t| t.is_dir()) {
                candidates.push((entry.into_path(), root.clone()));
            }
        }
    }
    candidates.sort();
    candidates.dedup_by(|a, b| a.0 == b.0);

    let mut projects: Vec<Project> = candidates
        .par_iter()
        .filter_map(|(dir, root)| build_project(dir, root))
        .collect();
    projects.sort_by(|a, b| natural_cmp(&a.id, &b.id));
    crate::workspace::assign(&mut projects);
    for p in &mut projects {
        crate::ports::annotate(p);
    }
    projects
}

/// Case-insensitive order where digit runs compare by value: `app2` < `App10`.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(ch) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(ch);
                        it.next();
                    }
                    s
                };
                let (n, m) = (take(&mut x), take(&mut y));
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = n.len().cmp(&m.len()).then_with(|| n.cmp(m));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

fn build_project(dir: &Path, root: &Path) -> Option<Project> {
    let detections = detect_dir(dir, root);
    if detections.is_empty() {
        return None;
    }
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_string_lossy().into_owned());
    let rel = dir.strip_prefix(root).unwrap_or(dir);
    let id = if rel.as_os_str().is_empty() {
        name.clone()
    } else {
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    };
    // One check instead of two lookups each in `git::branch` and `git::default_branch`.
    let repo = dir.join(".git").exists();
    let mut ecosystems = Vec::new();
    let mut tasks = Vec::new();
    let mut warnings = Vec::new();
    let mut hints = Vec::new();
    let mut detected_from = Vec::new();
    for d in detections {
        if !d.from.is_empty() {
            detected_from.push(crate::model::Source {
                ecosystem: d.ecosystem,
                from: d.from.clone(),
            });
        }
        ecosystems.push(d.ecosystem);
        tasks.extend(d.tasks);
        warnings.extend(d.warnings);
        hints.extend(d.hints);
    }
    Some(Project {
        id,
        name,
        path: dir.to_path_buf(),
        ecosystems,
        branch: repo.then(|| git::branch(dir)).flatten(),
        default_branch: repo.then(|| git::default_branch(dir)).flatten(),
        tasks,
        warnings,
        hints,
        detected_from,
        member_of: None,
    })
}

/// Absolute path without Windows `\\?\` verbatim prefixes.
fn dunce(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let s = abs.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => abs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn natural_order() {
        let mut v = vec!["App10", "app2", "beta", "Alpha"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["Alpha", "app2", "App10", "beta"]);
    }

    #[test]
    fn finds_nested_polyglot_projects_and_skips_node_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        fs::create_dir_all(r.join("apps/web/node_modules/dep")).unwrap();
        fs::write(
            r.join("apps/web/package.json"),
            r#"{"scripts":{"dev":"vite"}}"#,
        )
        .unwrap();
        fs::write(r.join("apps/web/node_modules/dep/package.json"), "{}").unwrap();
        fs::create_dir_all(r.join("services/api")).unwrap();
        fs::write(r.join("services/api/go.mod"), "module api").unwrap();
        fs::write(r.join("justfile"), "build:\n  echo\n").unwrap();

        let opts = DiscoverOptions {
            roots: vec![r.to_path_buf()],
            ..Default::default()
        };
        let ids: Vec<String> = discover(&opts).into_iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), 3, "{ids:?}");
        assert!(ids.contains(&"apps/web".to_string()));
        assert!(ids.contains(&"services/api".to_string()));
    }
}
