//! `kiwa setup`: the steps that get a freshly cloned project running, worked out from its files.
//! Static like discovery: only file checks; the steps themselves run later, once confirmed.
//!
//! The plan holds what the project declares or implies: installing dependencies with the
//! detected manager, creating `.env` from its example, and the project's own `setup` or
//! `bootstrap` tasks. Steps already done (dependencies present, `.env` there) are kept, marked
//! done, so the plan explains itself. README steps are not part of it; they are offered apart.

use crate::model::{Ecosystem, Project, Task};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Action {
    /// Run a task in `cwd`.
    Run { task: Task, cwd: PathBuf },
    /// Copy a file kiwa writes itself (never over an existing one).
    Copy { from: PathBuf, to: PathBuf },
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    /// What the step is for: `install dependencies`, `create .env`, `run setup`.
    pub what: String,
    /// Why it is in the plan, or why it is already done.
    pub why: String,
    pub done: bool,
    /// Whether `done` was checked from a file (`node_modules`, `.venv`, `vendor/`, `.env`).
    /// Without one (`cargo fetch`, `bundle install`), the step is in the plan but does not make
    /// a project "not set up".
    #[serde(skip)]
    pub checked: bool,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    pub project: String,
    pub steps: Vec<Step>,
    /// README steps, offered apart: run one by one, each confirmed.
    pub suggested: Vec<Task>,
}

impl Plan {
    /// Steps still to do.
    pub fn pending(&self) -> impl Iterator<Item = &Step> {
        self.steps.iter().filter(|s| !s.done)
    }
}

fn run(what: &str, why: String, done: Option<bool>, task: Task, cwd: &Path) -> Step {
    Step {
        what: what.into(),
        why,
        done: done.unwrap_or(false),
        checked: done.is_some(),
        action: Action::Run {
            task,
            cwd: cwd.to_path_buf(),
        },
    }
}

/// An ecosystem, the file it needs, the tool, its install argv, and what shows it is done.
type Install = (
    Ecosystem,
    &'static str,
    &'static str,
    &'static [&'static str],
    Option<&'static str>,
);

/// Dependency installs, one per ecosystem that has a lockfile or a manifest to install from.
/// `root` is where the install runs: the monorepo root for a member, the project otherwise.
fn installs(p: &Project, root: &Path) -> Vec<Step> {
    let at = |f: &str| root.join(f).exists();
    let mut out = Vec::new();
    let has = |e: Ecosystem| p.ecosystems.contains(&e);
    if has(Ecosystem::Node) && at("package.json") {
        let pkg = std::fs::read_to_string(root.join("package.json"))
            .ok()
            .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default();
        let pm = crate::detect::node_package_manager(root, root, &pkg);
        let argv: &[&str] = match pm {
            // A lockfile means a reproducible install.
            "npm" if at("package-lock.json") || at("npm-shrinkwrap.json") => &["npm", "ci"],
            "npm" => &["npm", "install"],
            "pnpm" => &["pnpm", "install"],
            "yarn" => &["yarn", "install"],
            _ => &["bun", "install"],
        };
        let done = at("node_modules") || at(".pnp.cjs");
        let why = if done {
            "node_modules is there".to_owned()
        } else {
            format!("{pm} dependencies are not installed")
        };
        out.push(run(
            "install dependencies",
            why,
            Some(done),
            Task::new("install", pm, argv),
            root,
        ));
    }
    if has(Ecosystem::Python) {
        let python: Option<(&str, &[&str])> = if at("uv.lock") {
            Some(("uv", &["uv", "sync"]))
        } else if at("poetry.lock") {
            Some(("poetry", &["poetry", "install"]))
        } else if at("pdm.lock") {
            Some(("pdm", &["pdm", "install"]))
        } else if at("Pipfile") {
            Some(("pipenv", &["pipenv", "install"]))
        } else {
            None
        };
        if let Some((tool, argv)) = python {
            let done = at(".venv");
            let why = if done {
                ".venv is there".into()
            } else {
                format!("{tool} environment not created")
            };
            out.push(run(
                "install dependencies",
                why,
                Some(done),
                Task::new("install", tool, argv),
                root,
            ));
        }
    }
    let simple: &[Install] = &[
        (
            Ecosystem::Php,
            "composer.json",
            "composer",
            &["composer", "install"],
            Some("vendor"),
        ),
        (
            Ecosystem::Ruby,
            "Gemfile",
            "bundle",
            &["bundle", "install"],
            None,
        ),
        (
            Ecosystem::Go,
            "go.mod",
            "go",
            &["go", "mod", "download"],
            None,
        ),
        (
            Ecosystem::Rust,
            "Cargo.toml",
            "cargo",
            &["cargo", "fetch"],
            None,
        ),
        (
            Ecosystem::Dotnet,
            "",
            "dotnet",
            &["dotnet", "restore"],
            Some("obj"),
        ),
    ];
    for (eco, file, tool, argv, marker) in simple {
        if !has(*eco) || (!file.is_empty() && !at(file)) {
            continue;
        }
        let done = marker.is_some_and(&at);
        let why = match (done, marker) {
            (true, Some(m)) => format!("{m}/ is there"),
            _ => format!("fetch the {tool} dependencies"),
        };
        out.push(run(
            "install dependencies",
            why,
            marker.map(|_| done),
            Task::new("install", tool, argv),
            root,
        ));
    }
    if has(Ecosystem::Mise) || at(".tool-versions") {
        out.push(run(
            "install the tool versions",
            "mise.toml / .tool-versions pin them".into(),
            None,
            Task::new("install", "mise", &["mise", "install"]),
            root,
        ));
    }
    out
}

/// The example environment files projects ship, most common first.
const ENV_EXAMPLES: &[&str] = &[".env.example", ".env.sample", ".env.template", ".env.dist"];

/// Task names that set a project up.
const SETUP_TASKS: &[&str] = &["setup", "bootstrap", "init", "db:setup"];

/// The plan for `p`. For a monorepo member, dependencies install at its root.
pub fn plan(p: &Project, readme: bool) -> Plan {
    let root = p
        .member_of
        .as_ref()
        .map_or(p.path.as_path(), |m| m.path.as_path());
    let mut steps = installs(p, root);
    if let Some(example) = ENV_EXAMPLES.iter().find(|f| p.path.join(f).is_file()) {
        let to = p.path.join(".env");
        let done = to.exists();
        steps.push(Step {
            what: "create .env".into(),
            why: if done {
                ".env is there".into()
            } else {
                format!("from {example}; fill in its values afterwards")
            },
            done,
            checked: true,
            action: Action::Copy {
                from: p.path.join(example),
                to,
            },
        });
    }
    for name in SETUP_TASKS {
        if let Some(t) = p.tasks.iter().find(|t| t.name == *name && !t.suggested) {
            steps.push(run(
                &format!("run {}", t.name),
                format!("declared by {}", t.runner),
                None,
                t.clone(),
                &p.path,
            ));
        }
    }
    let suggested = if readme {
        p.tasks.iter().filter(|t| t.suggested).cloned().collect()
    } else {
        Vec::new()
    };
    Plan {
        project: p.id.clone(),
        steps,
        suggested,
    }
}

/// Whether a project still needs setting up: a dependency install or `.env` is missing.
/// Declared setup tasks alone do not count (nothing tells whether they already ran).
pub fn needed(p: &Project) -> bool {
    plan(p, false).pending().any(|s| s.checked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn project(dir: &Path) -> Project {
        let mut ps = crate::discover(&crate::DiscoverOptions {
            roots: vec![dir.to_path_buf()],
            max_depth: 0,
            ..Default::default()
        });
        ps.remove(0)
    }

    #[test]
    fn a_fresh_node_project_with_an_env_example_and_a_setup_task() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("package.json"),
            r#"{"scripts":{"setup":"node seed.js","dev":"vite"},"dependencies":{"x":"1"}}"#,
        )
        .unwrap();
        fs::write(d.path().join("package-lock.json"), "{}").unwrap();
        fs::write(d.path().join(".env.example"), "PORT=3000\n").unwrap();
        fs::write(
            d.path().join("README.md"),
            "## Getting started\n```sh\nnpm run dev\n```\n",
        )
        .unwrap();
        let p = project(d.path());
        assert!(needed(&p));
        let plan = plan(&p, true);
        let what: Vec<_> = plan.steps.iter().map(|s| s.what.as_str()).collect();
        assert_eq!(what, ["install dependencies", "create .env", "run setup"]);
        match &plan.steps[0].action {
            Action::Run { task, cwd } => {
                assert_eq!(
                    task.command,
                    ["npm", "ci"],
                    "a lockfile: a reproducible install"
                );
                assert_eq!(cwd, &p.path);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(plan.suggested.len(), 1, "README steps are offered apart");
        let json = serde_json::to_value(&plan).unwrap();
        assert_eq!(json["steps"][1]["kind"], "copy");
        assert_eq!(json["steps"][0]["kind"], "run");

        // Once installed and with a .env, nothing is needed and both steps say why.
        fs::create_dir_all(d.path().join("node_modules")).unwrap();
        fs::write(d.path().join(".env"), "PORT=1\n").unwrap();
        let p = project(d.path());
        assert!(!needed(&p));
        let plan = plan_of(&p);
        assert!(plan.steps[0].done && plan.steps[1].done);
        assert_eq!(plan.pending().count(), 1, "only the declared setup task");
    }

    fn plan_of(p: &Project) -> Plan {
        plan(p, false)
    }

    #[test]
    fn other_ecosystems_and_monorepo_members() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("pyproject.toml"), "[project]\nname='x'\n").unwrap();
        fs::write(d.path().join("uv.lock"), "").unwrap();
        fs::write(d.path().join("composer.json"), "{}").unwrap();
        fs::create_dir_all(d.path().join("vendor")).unwrap();
        fs::write(d.path().join("go.mod"), "module x\n").unwrap();
        fs::write(d.path().join(".tool-versions"), "node 22\n").unwrap();
        let p = project(d.path());
        let steps: Vec<(String, bool)> = plan_of(&p)
            .steps
            .iter()
            .map(|s| match &s.action {
                Action::Run { task, .. } => (task.command.join(" "), s.done),
                Action::Copy { .. } => ("copy".into(), s.done),
            })
            .collect();
        assert_eq!(
            steps,
            [
                ("uv sync".to_owned(), false),
                ("composer install".into(), true),
                ("go mod download".into(), false),
                ("mise install".into(), false),
            ]
        );

        // A pnpm monorepo member installs at its root.
        let m = tempfile::tempdir().unwrap();
        fs::write(m.path().join("package.json"), r#"{"name":"root"}"#).unwrap();
        fs::write(m.path().join("pnpm-lock.yaml"), "").unwrap();
        fs::write(
            m.path().join("pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n",
        )
        .unwrap();
        fs::create_dir_all(m.path().join("apps/web")).unwrap();
        fs::write(m.path().join("apps/web/package.json"), r#"{"name":"web"}"#).unwrap();
        let ps = crate::discover(&crate::DiscoverOptions {
            roots: vec![m.path().to_path_buf()],
            ..Default::default()
        });
        let web = ps.iter().find(|p| p.id.ends_with("apps/web")).unwrap();
        match &plan_of(web).steps[0].action {
            Action::Run { task, cwd } => {
                assert_eq!(task.command, ["pnpm", "install"]);
                assert_eq!(cwd, &web.member_of.as_ref().unwrap().path);
            }
            other => panic!("{other:?}"),
        }
    }
}
