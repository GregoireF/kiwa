// SPDX-License-Identifier: MIT OR Apache-2.0

//! `kiwa about`: what a project is and how to start it, from its files alone.
//!
//! Identity comes from the manifests (`package.json`, `Cargo.toml`, `pyproject.toml`,
//! `composer.json`), then the README's first sentence and `.git/config`. "How to start" names
//! the tasks a newcomer looks for (install, dev, test, build, lint), the `setup` plan, and the
//! tool versions the project pins.

use crate::model::Project;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct About {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub stack: Vec<String>,
    /// The task for each role a newcomer looks for, by name: `install`, `dev`, `test`, `build`,
    /// `lint` (only those found).
    pub start: Vec<Role>,
    /// Steps `kiwa setup` would still run.
    pub setup: Vec<String>,
    /// Tool versions the project asks for.
    pub requires: Vec<Requirement>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Role {
    pub role: &'static str,
    pub task: String,
    pub runner: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Requirement {
    pub tool: String,
    pub version: String,
    pub from: String,
}

fn read(dir: &Path, f: &str) -> Option<String> {
    fs::read_to_string(dir.join(f))
        .ok()
        .map(|s| s.trim_start_matches('\u{feff}').to_owned())
}

fn json(dir: &Path, f: &str) -> Option<Value> {
    serde_json::from_str(&crate::detect::jsonc(&read(dir, f)?)).ok()
}

fn toml(dir: &Path, f: &str) -> Option<toml::Table> {
    read(dir, f)?.parse().ok()
}

/// A string field, or the `url`/`text` of an object (`repository`, `license`).
fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o
            .get("url")
            .or_else(|| o.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
    .filter(|s| !s.trim().is_empty())
}

fn toml_str(t: Option<&toml::Value>) -> Option<String> {
    match t? {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Table(o) => o.get("text").and_then(|v| v.as_str()).map(str::to_owned),
        _ => None,
    }
    .filter(|s| !s.trim().is_empty())
}

/// The README's first sentence: the first prose line, past headings, badges, HTML and lists.
pub fn readme_sentence(text: &str) -> Option<String> {
    let mut in_code = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        let skip = in_code
            || t.is_empty()
            || t.starts_with(['#', '<', '!', '[', '|', '>', '='])
            || t.starts_with("- ")
            || t.starts_with("* ")
            || t.starts_with("+ ")
            || t.chars().all(|c| c == '-' || c == '=');
        if skip {
            continue;
        }
        let plain: String = t.replace("**", "").replace('`', "").replace(['[', ']'], "");
        let end = plain
            .char_indices()
            .find(|&(i, c)| {
                matches!(c, '.' | '!' | '?')
                    && plain[i + c.len_utf8()..]
                        .chars()
                        .next()
                        .is_none_or(char::is_whitespace)
            })
            .map_or(plain.len(), |(i, c)| i + c.len_utf8());
        let s = plain[..end].trim();
        return (s.len() > 3).then(|| s.to_owned());
    }
    None
}

/// `url` of `[remote "origin"]` in `.git/config`, as an https address when it is an SSH one.
pub fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_origin = t == "[remote \"origin\"]";
            continue;
        }
        if in_origin
            && let Some(url) = t
                .strip_prefix("url")
                .map(str::trim_start)
                .and_then(|r| r.strip_prefix('='))
        {
            let url = url.trim();
            // git@github.com:owner/repo.git → https://github.com/owner/repo
            let url = match url.strip_prefix("git@").and_then(|r| r.split_once(':')) {
                Some((host, path)) => format!("https://{host}/{path}"),
                None => url.to_owned(),
            };
            return Some(url.trim_end_matches(".git").to_owned());
        }
    }
    None
}

/// Tool versions pinned by version files and manifests.
fn requirements(dir: &Path, pkg: Option<&Value>, cargo: Option<&toml::Table>) -> Vec<Requirement> {
    let mut out: Vec<Requirement> = Vec::new();
    let mut add = |tool: &str, version: &str, from: &str| {
        let v = version.trim().trim_start_matches('v');
        if !v.is_empty() && !out.iter().any(|r| r.tool == tool) {
            out.push(Requirement {
                tool: tool.into(),
                version: v.into(),
                from: from.into(),
            });
        }
    };
    for f in [".nvmrc", ".node-version"] {
        if let Some(v) = read(dir, f) {
            add("node", v.lines().next().unwrap_or_default(), f);
        }
    }
    if let Some(engines) = pkg
        .and_then(|p| p.get("engines"))
        .and_then(Value::as_object)
    {
        for (tool, v) in engines {
            if let Some(v) = v.as_str() {
                add(tool, v, "package.json engines");
            }
        }
    }
    if let Some(v) = read(dir, ".python-version") {
        add(
            "python",
            v.lines().next().unwrap_or_default(),
            ".python-version",
        );
    }
    if let Some(v) =
        toml(dir, "pyproject.toml").and_then(|t| toml_str(t.get("project")?.get("requires-python")))
    {
        add("python", &v, "pyproject.toml requires-python");
    }
    if let Some(v) =
        toml(dir, "rust-toolchain.toml").and_then(|t| toml_str(t.get("toolchain")?.get("channel")))
    {
        add("rust", &v, "rust-toolchain.toml");
    } else if let Some(v) = read(dir, "rust-toolchain") {
        add(
            "rust",
            v.lines().next().unwrap_or_default(),
            "rust-toolchain",
        );
    }
    if let Some(v) = cargo.and_then(|c| toml_str(c.get("package")?.get("rust-version"))) {
        add("rust", &format!(">={v}"), "Cargo.toml rust-version");
    }
    if let Some(go) = read(dir, "go.mod") {
        for line in go.lines() {
            if let Some(v) = line.strip_prefix("toolchain go") {
                add("go", v, "go.mod toolchain");
            } else if let Some(v) = line.strip_prefix("go ") {
                add("go", &format!(">={}", v.trim()), "go.mod");
            }
        }
    }
    if let Some(tv) = read(dir, ".tool-versions") {
        for line in tv.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
            let mut w = line.split_whitespace();
            if let (Some(tool), Some(v)) = (w.next(), w.next()) {
                add(tool, v, ".tool-versions");
            }
        }
    }
    for f in ["mise.toml", ".mise.toml"] {
        if let Some(tools) = toml(dir, f).and_then(|t| t.get("tools")?.as_table().cloned()) {
            for (tool, v) in &tools {
                if let Some(v) = v.as_str() {
                    add(tool, v, f);
                }
            }
        }
    }
    out
}

/// The task names that play each role, most usual first.
const ROLES: &[(&str, &[&str])] = &[
    ("install", &["install", "setup", "bootstrap"]),
    ("dev", &["dev", "start", "serve", "run", "watch"]),
    ("test", &["test", "tests", "check"]),
    ("build", &["build", "compile"]),
    ("lint", &["lint", "clippy", "check"]),
];

pub fn about(p: &Project) -> About {
    let dir = p.path.as_path();
    let pkg = json(dir, "package.json");
    let cargo = toml(dir, "Cargo.toml");
    let py = toml(dir, "pyproject.toml");
    let composer = json(dir, "composer.json");
    let cargo_pkg = cargo.as_ref().and_then(|c| c.get("package"));
    let py_project = py.as_ref().and_then(|t| t.get("project"));
    let description = text(pkg.as_ref().and_then(|v| v.get("description")))
        .or_else(|| toml_str(cargo_pkg.and_then(|c| c.get("description"))))
        .or_else(|| toml_str(py_project.and_then(|c| c.get("description"))))
        .or_else(|| text(composer.as_ref().and_then(|v| v.get("description"))))
        .or_else(|| {
            ["README.md", "readme.md", "README"]
                .iter()
                .find_map(|f| read(dir, f))
                .and_then(|t| readme_sentence(&t))
        });
    let license = text(pkg.as_ref().and_then(|v| v.get("license")))
        .or_else(|| toml_str(cargo_pkg.and_then(|c| c.get("license"))))
        .or_else(|| toml_str(py_project.and_then(|c| c.get("license"))))
        .or_else(|| text(composer.as_ref().and_then(|v| v.get("license"))));
    let py_url = py_project
        .and_then(|c| c.get("urls"))
        .and_then(|u| u.as_table())
        .and_then(|u| {
            ["Repository", "Source", "repository", "source", "Homepage"]
                .iter()
                .find_map(|k| u.get(*k)?.as_str().map(str::to_owned))
        });
    let repository = text(pkg.as_ref().and_then(|v| v.get("repository")))
        .or_else(|| toml_str(cargo_pkg.and_then(|c| c.get("repository"))))
        .or(py_url)
        .or_else(|| read(dir, ".git/config").and_then(|c| origin_url(&c)))
        .map(|u| u.trim_start_matches("git+").to_owned());
    let start = ROLES
        .iter()
        .filter_map(|(role, names)| {
            names.iter().find_map(|n| {
                p.tasks
                    .iter()
                    .find(|t| t.name == *n && !t.suggested)
                    .map(|t| Role {
                        role,
                        task: t.name.clone(),
                        runner: t.runner.clone(),
                    })
            })
        })
        .collect();
    // Only what is known to be missing (`node_modules`, `.env`…), as `setup::needed` decides.
    let setup = crate::setup::plan(p, false)
        .pending()
        .filter(|s| s.checked)
        .map(|s| s.what.clone())
        .collect();
    About {
        id: p.id.clone(),
        name: text(pkg.as_ref().and_then(|v| v.get("name")))
            .or_else(|| toml_str(cargo_pkg.and_then(|c| c.get("name"))))
            .or_else(|| toml_str(py_project.and_then(|c| c.get("name"))))
            .unwrap_or_else(|| p.name.clone()),
        description,
        license,
        repository,
        branch: p.branch.clone(),
        stack: p.ecosystems.iter().map(|e| e.label().to_owned()).collect(),
        start,
        setup,
        requires: requirements(dir, pkg.as_ref(), cargo.as_ref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_first_sentence() {
        let t = "<p align=center><img src=x></p>\n\n# kiwa 🥝\n\n[![ci](x)](y)\n\n> quote\n\n**Find any project.** Run any task. More.\n";
        assert_eq!(readme_sentence(t).as_deref(), Some("Find any project."));
        assert_eq!(
            readme_sentence("# x\n\nA tool for v1.2 users\n").as_deref(),
            Some("A tool for v1.2 users"),
            "a dot inside a word does not end the sentence"
        );
        assert_eq!(readme_sentence("# only a title\n```\ncode.\n```\n"), None);
    }

    #[test]
    fn origin_urls() {
        let c = "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = https://x/u\n[remote \"origin\"]\n\turl = git@github.com:owner/repo.git\n";
        assert_eq!(
            origin_url(c).as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(
            origin_url("[remote \"origin\"]\nurl=https://gitlab.com/a/b.git\n").as_deref(),
            Some("https://gitlab.com/a/b")
        );
        assert_eq!(origin_url("[core]\n"), None);
    }

    #[test]
    fn identity_start_and_requirements() {
        let d = tempfile::tempdir().unwrap();
        let w = |f: &str, s: &str| {
            let p = d.path().join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, s).unwrap();
        };
        w(
            "package.json",
            r#"{"name":"shop","description":"The shop.","license":"MIT",
               "repository":{"type":"git","url":"git+https://github.com/x/shop.git"},
               "engines":{"node":">=20","pnpm":"9"},
               "scripts":{"start":"node .","test":"vitest","build":"tsc","lint":"eslint ."}}"#,
        );
        w("pnpm-lock.yaml", "");
        w(".nvmrc", "v22.11.0\n");
        w(".tool-versions", "# pins\nterraform 1.9.0\nnode 18\n");
        w(".env.example", "X=1\n");
        let p = crate::discover(&crate::DiscoverOptions {
            roots: vec![d.path().to_path_buf()],
            max_depth: 0,
            ..Default::default()
        })
        .remove(0);
        let a = about(&p);
        assert_eq!(a.name, "shop");
        assert_eq!(a.description.as_deref(), Some("The shop."));
        assert_eq!(a.license.as_deref(), Some("MIT"));
        assert_eq!(
            a.repository.as_deref(),
            Some("https://github.com/x/shop.git")
        );
        let roles: Vec<(&str, &str)> = a.start.iter().map(|r| (r.role, r.task.as_str())).collect();
        assert_eq!(
            roles,
            [
                ("dev", "start"),
                ("test", "test"),
                ("build", "build"),
                ("lint", "lint")
            ]
        );
        assert_eq!(
            a.setup,
            ["install dependencies", "create .env"],
            "only what is known to be missing (`mise install` cannot be checked)"
        );
        let req: Vec<String> = a
            .requires
            .iter()
            .map(|r| format!("{} {} ({})", r.tool, r.version, r.from))
            .collect();
        assert_eq!(
            req,
            [
                "node 22.11.0 (.nvmrc)",
                "pnpm 9 (package.json engines)",
                "terraform 1.9.0 (.tool-versions)"
            ],
            "the first source wins for each tool"
        );

        let r = tempfile::tempdir().unwrap();
        fs::write(
            r.path().join("Cargo.toml"),
            "[package]\nname = \"crab\"\nrust-version = \"1.85\"\nlicense = \"MIT OR Apache-2.0\"\n",
        )
        .unwrap();
        fs::write(
            r.path().join("README.md"),
            "# crab\n\nA small crab. It pinches.\n",
        )
        .unwrap();
        fs::write(
            r.path().join("go.mod"),
            "module x\n\ngo 1.22\ntoolchain go1.22.4\n",
        )
        .unwrap();
        let p = crate::discover(&crate::DiscoverOptions {
            roots: vec![r.path().to_path_buf()],
            max_depth: 0,
            ..Default::default()
        })
        .remove(0);
        let a = about(&p);
        assert_eq!(a.description.as_deref(), Some("A small crab."));
        assert!(
            a.requires
                .iter()
                .any(|q| q.tool == "rust" && q.version == ">=1.85")
        );
        assert!(
            a.requires
                .iter()
                .any(|q| q.tool == "go" && q.version == ">=1.22"),
            "go directive first"
        );
    }
}
