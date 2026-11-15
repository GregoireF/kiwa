//! Static detectors. Each one reads manifests in a single directory and returns
//! the ecosystem plus the tasks it exposes. Detectors never execute anything.

use crate::model::{Ecosystem, Param, ParamForm, Separator, Task};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub struct Detection {
    pub ecosystem: Ecosystem,
    pub tasks: Vec<Task>,
    /// Problems found in the manifest. The project is still listed, never silently dropped.
    pub warnings: Vec<String>,
    pub hints: Vec<String>,
    /// Why it was detected: the manifest file, or for JavaScript what chose the package manager.
    pub from: String,
}

/// The files that make each ecosystem detected, most specific first (for `from`).
const SOURCES: &[(Ecosystem, &[&str])] = &[
    (Ecosystem::Deno, &["deno.json", "deno.jsonc"]),
    (Ecosystem::Rust, &["Cargo.toml"]),
    (Ecosystem::Go, &["go.mod"]),
    (
        Ecosystem::Python,
        &[
            "uv.lock",
            "poetry.lock",
            "pdm.lock",
            "pyproject.toml",
            "requirements.txt",
            "setup.py",
            "Pipfile",
        ],
    ),
    (Ecosystem::Just, &["justfile", "Justfile", ".justfile"]),
    (
        Ecosystem::Task,
        &[
            "Taskfile.yml",
            "Taskfile.yaml",
            "taskfile.yml",
            "taskfile.yaml",
        ],
    ),
    (
        Ecosystem::Mise,
        &["mise.toml", ".mise.toml", "mise-tasks", ".mise/tasks"],
    ),
    (Ecosystem::Make, &["Makefile", "makefile", "GNUmakefile"]),
    (
        Ecosystem::Gradle,
        &[
            "gradlew",
            "build.gradle.kts",
            "build.gradle",
            "settings.gradle.kts",
            "settings.gradle",
        ],
    ),
    (Ecosystem::Maven, &["mvnw", "pom.xml"]),
    (Ecosystem::Php, &["composer.json"]),
    (
        Ecosystem::Ruby,
        &["Gemfile", "Rakefile", "rakefile", "Rakefile.rb"],
    ),
    (
        Ecosystem::Compose,
        &[
            "compose.yaml",
            "compose.yml",
            "docker-compose.yml",
            "docker-compose.yaml",
        ],
    ),
    (Ecosystem::Git, &[".git"]),
];

fn default_source(eco: Ecosystem, dir: &Path) -> String {
    if eco == Ecosystem::Dotnet
        && let Ok(rd) = fs::read_dir(dir)
    {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let l = n.to_lowercase();
            if l.ends_with(".sln") || l.ends_with(".slnx") || l.ends_with("proj") {
                return n;
            }
        }
    }
    SOURCES
        .iter()
        .find(|(e, _)| *e == eco)
        .and_then(|(_, files)| files.iter().find(|f| dir.join(f).exists()))
        .map(|f| (*f).to_owned())
        .unwrap_or_default()
}

type Detector = fn(&Path, &Path) -> Option<Detection>;

/// Order matters: it is the display order and the tie-break for duplicate task names.
const DETECTORS: &[Detector] = &[
    node, deno, rust, go, python, just, taskfile, mise, make, gradle, maven, dotnet, php, ruby,
    compose,
];

/// Nx targets of `project.json`, run through Nx (`npx nx run <project>:<target>`), with the
/// targets that `nx.json`'s `targetDefaults` give every project. Targets inferred by Nx plugins
/// exist only once Nx runs, so they are not listed.
fn nx(dir: &Path, root: &Path) -> Option<Detection> {
    let text = read(dir, "project.json")?;
    let project: Value = match serde_json::from_str(&jsonc(&text)) {
        Ok(v) => v,
        Err(e) => return broken(Ecosystem::Nx, "project.json", format!("invalid JSON ({e})")),
    };
    let name = project
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))?;
    let mut targets: Vec<(String, Option<String>)> = project
        .get("targets")
        .and_then(Value::as_object)
        .map(|t| {
            t.iter()
                .map(|(k, v)| {
                    let cmd = v
                        .get("options")
                        .and_then(|o| o.get("command"))
                        .or_else(|| v.get("command"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| v.get("executor").and_then(Value::as_str).map(str::to_owned));
                    (k.clone(), cmd)
                })
                .collect()
        })
        .unwrap_or_default();
    // `nx.json` at a parent up to the scan root: defaults that apply to every project.
    if let Some(defaults) = up_to_root(dir, root)
        .find_map(|d| read(d, "nx.json"))
        .and_then(|t| serde_json::from_str::<Value>(&jsonc(&t)).ok())
    {
        for (k, _) in defaults
            .get("targetDefaults")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            // Keys may be executor names (`@nx/vite:build`); only plain target names apply.
            if !k.contains(':') && !targets.iter().any(|(t, _)| t == k) {
                targets.push((k.clone(), None));
            }
        }
    }
    let tasks = targets
        .into_iter()
        .map(|(target, desc)| {
            let spec = format!("{name}:{target}");
            Task::new(&target, "nx", &["npx", "nx", "run", &spec]).with_description(desc)
        })
        .collect();
    let mut d = found(Ecosystem::Nx, tasks)?;
    d.from = "project.json".into();
    Some(d)
}

/// Turborepo tasks (`tasks` in turbo 2, `pipeline` in turbo 1), run with `npx turbo run`.
/// `web#build` runs `build` for `web` only; `//#lint` is a task of the root package.
fn turbo(dir: &Path, _: &Path) -> Option<Detection> {
    let text = read(dir, "turbo.json")?;
    let v: Value = match serde_json::from_str(&jsonc(&text)) {
        Ok(v) => v,
        Err(e) => {
            return broken(
                Ecosystem::Turbo,
                "turbo.json",
                format!("invalid JSON ({e})"),
            );
        }
    };
    let tasks = v
        .get("tasks")
        .or_else(|| v.get("pipeline"))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(key, _)| match key.split_once('#') {
            Some(("//", task)) => Task::new(
                format!("turbo:{task}"),
                "turbo",
                &["npx", "turbo", "run", task],
            ),
            Some((package, task)) => Task::new(
                format!("turbo:{package}#{task}"),
                "turbo",
                &["npx", "turbo", "run", task, "--filter", package],
            ),
            None => Task::new(
                format!("turbo:{key}"),
                "turbo",
                &["npx", "turbo", "run", key],
            )
            .with_description(Some(format!("`{key}` in every package that has it"))),
        })
        .collect();
    let mut d = found(Ecosystem::Turbo, tasks)?;
    d.from = "turbo.json".into();
    Some(d)
}

/// Run every detector on `dir`. `root` is the scan root (used to look up lockfiles
/// in parent directories of monorepo packages).
pub fn detect_dir(dir: &Path, root: &Path) -> Vec<Detection> {
    let mut out: Vec<Detection> = DETECTORS.iter().filter_map(|d| d(dir, root)).collect();
    // The newer sources look at the folder's listing first, read once, and open only what is
    // there: most folders of a large tree have none of them (budget: `cargo xtask bench`).
    let names: std::collections::HashSet<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let has = |n: &str| names.contains(n);
    if has(".devcontainer") || has(".devcontainer.json") {
        out.extend(devcontainer(dir, root));
    }
    if has("project.json") {
        out.extend(nx(dir, root));
    }
    if has("turbo.json") {
        out.extend(turbo(dir, root));
    }
    let repo = has(".git");
    // Script folders, Procfile, editor and CI tasks add to a project; on their own they never
    // make one (`~/bin`, a stray `.vscode`).
    if !out.is_empty() || repo {
        if SCRIPT_DIRS.iter().any(|d| has(d)) {
            out.extend(scripts(dir));
        }
        if has("Procfile") {
            out.extend(crate::sources::procfile(dir));
        }
        if has(".vscode") {
            out.extend(crate::sources::vscode(dir));
        }
        if has(".github") || has(".gitlab-ci.yml") {
            out.extend(crate::sources::ci(dir));
        }
        if names.iter().any(|n| n.to_lowercase().starts_with("readme")) {
            out.extend(crate::sources::readme(dir));
        }
    }
    // A repository without any known manifest is still a project the user may want to find.
    const ADDERS: &[Ecosystem] = &[
        Ecosystem::Scripts,
        Ecosystem::Procfile,
        Ecosystem::Vscode,
        Ecosystem::Ci,
        Ecosystem::Readme,
    ];
    if repo && out.iter().all(|d| ADDERS.contains(&d.ecosystem)) {
        out.extend(found(Ecosystem::Git, Vec::new()));
    }
    for d in &mut out {
        if d.from.is_empty() {
            d.from = default_source(d.ecosystem, dir);
        }
    }
    out
}

/// Reads a text file, dropping a leading UTF-8 byte order mark (Windows editors add one).
fn read(dir: &Path, file: &str) -> Option<String> {
    let mut s = fs::read_to_string(dir.join(file)).ok()?;
    if s.starts_with('\u{feff}') {
        s.drain(..'\u{feff}'.len_utf8());
    }
    Some(s)
}

fn first_existing<'a>(dir: &Path, files: &[&'a str]) -> Option<&'a str> {
    files.iter().copied().find(|f| dir.join(f).is_file())
}

fn found(ecosystem: Ecosystem, tasks: Vec<Task>) -> Option<Detection> {
    Some(Detection {
        ecosystem,
        tasks,
        warnings: Vec::new(),
        hints: Vec::new(),
        from: String::new(),
    })
}

fn broken(ecosystem: Ecosystem, file: &str, err: impl std::fmt::Display) -> Option<Detection> {
    Some(Detection {
        ecosystem,
        tasks: Vec::new(),
        warnings: vec![format!("{file}: {err}")],
        hints: Vec::new(),
        from: file.to_owned(),
    })
}

// ---------------------------------------------------------------- JavaScript

fn node(dir: &Path, root: &Path) -> Option<Detection> {
    let pkg: Value = match serde_json::from_str(&read(dir, "package.json")?) {
        Ok(v @ Value::Object(_)) => v,
        Ok(_) => return broken(Ecosystem::Node, "package.json", "expected a JSON object"),
        Err(e) => {
            return broken(
                Ecosystem::Node,
                "package.json",
                format!("invalid JSON ({e})"),
            );
        }
    };
    let (pm, why) = node_package_manager_why(dir, root, &pkg);
    let berry = pm == "yarn" && yarn_berry(dir, root, &pkg);
    let hooks_run = node_runs_hooks(dir, root, pm, berry, &pkg);
    // Measured on npm 10.9, pnpm 9.0, Yarn 1.22 / 4.5, Bun 1.3 (docs/explanation/args-forwarding.md).
    let separator = match pm {
        "npm" => Separator::Always,
        "yarn" if !berry => Separator::Always,
        _ => Separator::Never,
    };
    let mut tasks = Vec::new();
    let mut warnings = Vec::new();
    match pkg.get("scripts") {
        None | Some(Value::Null) => {}
        Some(Value::Object(scripts)) => {
            let has = |n: &str| scripts.get(n).is_some_and(Value::is_string);
            for (name, cmd) in scripts {
                let Some(cmd) = cmd.as_str() else { continue };
                // When the package manager runs pre/post hooks itself, fold them into their main
                // script; otherwise keep them listed so they stay runnable on their own.
                let is_hook = ["pre", "post"].iter().any(|p| {
                    name.strip_prefix(p)
                        .is_some_and(|main| !main.is_empty() && has(main))
                });
                if is_hook && hooks_run {
                    continue;
                }
                let mut t =
                    Task::new(name, pm, &[pm, "run", name]).with_description(Some(cmd.to_owned()));
                t.separator = separator;
                t.params = script_params(cmd);
                if hooks_run {
                    t.hooks = ["pre", "post"]
                        .iter()
                        .map(|p| format!("{p}{name}"))
                        .filter(|h| has(h))
                        .collect();
                }
                tasks.push(t);
            }
        }
        Some(_) => warnings.push("package.json: \"scripts\" must be an object".to_owned()),
    }
    if pkg.get("packageManager").is_none() {
        warnings.extend(mixed_lockfiles(dir, root));
    }
    let mut hints = Vec::new();
    let has_deps = ["dependencies", "devDependencies"].iter().any(|k| {
        pkg.get(k)
            .and_then(Value::as_object)
            .is_some_and(|m| !m.is_empty())
    });
    if has_deps && !node_installed(dir, root) {
        hints.push(format!("dependencies not installed: run `{pm} install`"));
    }
    Some(Detection {
        ecosystem: Ecosystem::Node,
        tasks,
        warnings,
        hints,
        from: why,
    })
}

/// `node_modules` or a Yarn PnP loader in this folder or a parent up to the scan root.
fn node_installed(dir: &Path, root: &Path) -> bool {
    for ancestor in dir.ancestors() {
        if ancestor.join("node_modules").is_dir() || ancestor.join(".pnp.cjs").is_file() {
            return true;
        }
        if ancestor == root {
            break;
        }
    }
    false
}

/// Major version from `packageManager: "<name>@<major>..."`, when it names `name`.
fn pm_major(pkg: &Value, name: &str) -> Option<u32> {
    let v = pkg.get("packageManager")?.as_str()?;
    let ver = v.trim().strip_prefix(name)?.strip_prefix('@')?;
    ver.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// Does the package manager run `pre<x>`/`post<x>` automatically?
/// npm, Bun, Yarn 1: yes. Yarn 2+: no. pnpm: `enable-pre-post-scripts`, whose default is
/// `false` up to pnpm 8 and `true` from pnpm 9 (pnpm.io/8.x/cli/run vs pnpm.io/9.x/cli/run).
pub fn node_runs_hooks(dir: &Path, root: &Path, pm: &str, berry: bool, pkg: &Value) -> bool {
    match pm {
        "yarn" => !berry,
        "pnpm" => {
            if let Some(v) = npmrc_value(dir, root, "enable-pre-post-scripts") {
                return v == "true";
            }
            pm_major(pkg, "pnpm").is_none_or(|m| m >= 9)
        }
        _ => true,
    }
}

/// First `key=value` found in `.npmrc` from `dir` up to the scan root.
fn npmrc_value(dir: &Path, root: &Path, key: &str) -> Option<String> {
    for ancestor in dir.ancestors() {
        if let Some(text) = read(ancestor, ".npmrc") {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=')
                    && k.trim() == key
                {
                    return Some(v.trim().to_lowercase());
                }
            }
        }
        if ancestor == root {
            break;
        }
    }
    None
}

/// The folders from `dir` up to the scan root, nearest first.
fn up_to_root<'a>(dir: &'a Path, root: &'a Path) -> impl Iterator<Item = &'a Path> {
    let mut done = false;
    dir.ancestors().take_while(move |a| {
        let keep = !done;
        done = done || *a == root;
        keep
    })
}

/// What a `yarn.lock` says about its author, from its header and first entry. Classic lockfiles
/// announce themselves in a comment (`# yarn lockfile v1`); Yarn 2+ ones open with a
/// `__metadata:` entry (measured with Yarn 1.22 and 4.5). `None` when it says neither.
fn yarn_lock_is_modern(path: &Path) -> Option<bool> {
    use std::io::BufRead;
    let file = fs::File::open(path).ok()?;
    for line in std::io::BufReader::new(file).lines().take(40) {
        let line = line.ok()?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match line.strip_prefix('#') {
            Some(comment) if comment.contains("yarn lockfile v1") => return Some(false),
            Some(_) => continue,
            None => return Some(line.starts_with("__metadata:")),
        }
    }
    None
}

/// Yarn 2+ ("berry")? In order of reliability: the major version in `packageManager`, the
/// nearest `yarn.lock`, then a `.yarnrc.yml` (a file only Yarn 2+ reads). Yarn 1 otherwise.
fn yarn_berry(dir: &Path, root: &Path, pkg: &Value) -> bool {
    if let Some(major) = pm_major(pkg, "yarn") {
        return major >= 2;
    }
    let mut rc = false;
    for folder in up_to_root(dir, root) {
        let lock = folder.join("yarn.lock");
        if lock.is_file()
            && let Some(modern) = yarn_lock_is_modern(&lock)
        {
            return modern;
        }
        rc = rc || folder.join(".yarnrc.yml").is_file();
    }
    rc
}

/// `packageManager` field first, then the nearest lockfile up to the scan root.
pub fn node_package_manager(dir: &Path, root: &Path, pkg: &Value) -> &'static str {
    node_package_manager_why(dir, root, pkg).0
}

/// Lockfile → the package manager that writes it.
const LOCKFILES: &[(&str, &str)] = &[
    ("bun.lock", "bun"),
    ("bun.lockb", "bun"),
    ("npm-shrinkwrap.json", "npm"),
    ("package-lock.json", "npm"),
    ("pnpm-lock.yaml", "pnpm"),
    ("yarn.lock", "yarn"),
];

/// The lockfiles in `folder`, most recently written first: when a project switched package
/// managers and the old lockfile was left behind, the fresh one is the one in use.
fn lockfiles_in(folder: &Path) -> Vec<(&'static str, &'static str)> {
    let mut found: Vec<_> = LOCKFILES
        .iter()
        .filter_map(|&(file, pm)| {
            let meta = fs::metadata(folder.join(file))
                .ok()
                .filter(|m| m.is_file())?;
            Some((meta.modified().ok(), file, pm))
        })
        .collect();
    found.sort_by_key(|x| std::cmp::Reverse(x.0));
    found.into_iter().map(|(_, f, pm)| (f, pm)).collect()
}

/// A warning when one folder holds lockfiles of different package managers.
fn mixed_lockfiles(dir: &Path, root: &Path) -> Option<String> {
    let found = up_to_root(dir, root)
        .map(lockfiles_in)
        .find(|l| !l.is_empty())?;
    let used = found[0].1;
    let stale: Vec<&str> = found
        .iter()
        .filter(|(_, pm)| *pm != used)
        .map(|(f, _)| *f)
        .collect();
    (!stale.is_empty()).then(|| {
        format!(
            "lockfiles of several package managers: {used} is used ({} is the newest); consider deleting {}",
            found[0].0,
            stale.join(", ")
        )
    })
}

/// The package manager and why it was chosen: `packageManager: pnpm@9.1.0`, the lockfile
/// (with `../` when it sits in a parent folder of a monorepo package), or the npm default.
pub fn node_package_manager_why(dir: &Path, root: &Path, pkg: &Value) -> (&'static str, String) {
    if let Some(field) = pkg.get("packageManager").and_then(Value::as_str) {
        let name = field.split('@').next().unwrap_or_default();
        if let Some(&(_, pm)) = LOCKFILES.iter().find(|(_, pm)| *pm == name) {
            return (pm, format!("packageManager: {}", field.trim()));
        }
    }
    for (depth, folder) in up_to_root(dir, root).enumerate() {
        if let Some(&(file, pm)) = lockfiles_in(folder).first() {
            return (pm, format!("{}{file}", "../".repeat(depth)));
        }
    }
    ("npm", "no lockfile (npm by default)".to_owned())
}
fn deno(dir: &Path, _: &Path) -> Option<Detection> {
    let file = first_existing(dir, &["deno.json", "deno.jsonc"])?;
    let mut tasks = Vec::new();
    // deno.jsonc may contain comments; we then still report the project, without tasks.
    if let Some(v) = read(dir, file).and_then(|t| serde_json::from_str::<Value>(&t).ok())
        && let Some(obj) = v.get("tasks").and_then(Value::as_object)
    {
        for (name, cmd) in obj {
            let desc = cmd
                .as_str()
                .or_else(|| cmd.get("description").and_then(Value::as_str));
            tasks.push(
                Task::new(name, "deno", &["deno", "task", name])
                    .with_description(desc.map(str::to_owned)),
            );
        }
    }
    found(Ecosystem::Deno, tasks)
}

// ---------------------------------------------------------------- Rust / Go

fn rust(dir: &Path, _: &Path) -> Option<Detection> {
    let manifest: toml::Table = read(dir, "Cargo.toml")?.parse().ok()?;
    let is_workspace = manifest.contains_key("workspace");
    let has_bin = dir.join("src/main.rs").is_file() || manifest.contains_key("bin");
    let mut tasks = vec![
        Task::new("build", "cargo", &["cargo", "build"]),
        Task::new("test", "cargo", &["cargo", "test"]),
        Task::new("check", "cargo", &["cargo", "check"]),
        Task::new("clippy", "cargo", &["cargo", "clippy"]),
    ];
    if is_workspace && !manifest.contains_key("package") {
        for t in &mut tasks {
            t.command.push("--workspace".into());
        }
    }
    if has_bin {
        tasks.push(Task::new("run", "cargo", &["cargo", "run"]));
    }
    // Project-local cargo aliases (`.cargo/config.toml` [alias]).
    if let Some(cfg) = read(dir, ".cargo/config.toml").and_then(|t| t.parse::<toml::Table>().ok())
        && let Some(alias) = cfg.get("alias").and_then(|a| a.as_table())
    {
        for name in alias.keys() {
            tasks.push(Task::new(name, "cargo", &["cargo", name]));
        }
    }
    found(Ecosystem::Rust, tasks)
}

fn go(dir: &Path, _: &Path) -> Option<Detection> {
    dir.join("go.mod").is_file().then_some(())?;
    let mut tasks = vec![
        Task::new("build", "go", &["go", "build", "./..."]),
        Task::new("test", "go", &["go", "test", "./..."]),
        Task::new("vet", "go", &["go", "vet", "./..."]),
    ];
    if dir.join("main.go").is_file() {
        tasks.push(Task::new("run", "go", &["go", "run", "."]));
    }
    found(Ecosystem::Go, tasks)
}

// ---------------------------------------------------------------- Python

fn python(dir: &Path, _: &Path) -> Option<Detection> {
    let pyproject: Option<toml::Table> = read(dir, "pyproject.toml").and_then(|t| t.parse().ok());
    if pyproject.is_none()
        && first_existing(dir, &["requirements.txt", "setup.py", "Pipfile"]).is_none()
    {
        return None;
    }
    let py = pyproject.unwrap_or_default();
    let tool = py.get("tool").and_then(|t| t.as_table());
    let has_tool = |k: &str| tool.is_some_and(|t| t.contains_key(k));

    let runner = if dir.join("uv.lock").is_file() || has_tool("uv") {
        "uv"
    } else if dir.join("poetry.lock").is_file() || has_tool("poetry") {
        "poetry"
    } else if dir.join("pdm.lock").is_file() || has_tool("pdm") {
        "pdm"
    } else if has_tool("hatch") {
        "hatch"
    } else {
        "python"
    };
    let wrap = |args: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = match runner {
            "python" => vec![],
            r => vec![r.to_owned(), "run".to_owned()],
        };
        v.extend(args.iter().map(|s| (*s).to_owned()));
        v
    };
    let mut tasks = Vec::new();
    let mut push = |name: &str, argv: Vec<String>| {
        tasks.push(Task::from_argv(name, runner, argv));
    };

    if let Some(scripts) = py
        .get("project")
        .and_then(|p| p.get("scripts"))
        .and_then(|s| s.as_table())
    {
        for name in scripts.keys() {
            push(name, wrap(&[name]));
        }
    }
    if let Some(tool) = tool {
        let keys = |path: &[&str]| -> Vec<String> {
            let mut cur = Some(tool);
            for p in path {
                cur = cur.and_then(|t| t.get(*p)).and_then(|v| v.as_table());
            }
            cur.map(|t| t.keys().cloned().collect()).unwrap_or_default()
        };
        for name in keys(&["pdm", "scripts"]) {
            push(&name, vec!["pdm".into(), "run".into(), name.clone()]);
        }
        for name in keys(&["poe", "tasks"]) {
            push(&name, wrap(&["poe", &name]));
        }
        for name in keys(&["hatch", "envs", "default", "scripts"]) {
            push(&name, vec!["hatch".into(), "run".into(), name.clone()]);
        }
    }
    let has_tests = dir.join("tests").is_dir() || tool.is_some_and(|t| t.get("pytest").is_some());
    if has_tests {
        push("test", wrap(&["pytest"]));
    }
    // A user-defined `test` script wins over the implicit pytest one.
    if tasks.iter().filter(|t| t.name == "test").count() > 1 {
        tasks.pop();
    }
    found(Ecosystem::Python, tasks)
}

// ---------------------------------------------------------------- Generic runners

fn make(dir: &Path, _: &Path) -> Option<Detection> {
    let file = first_existing(dir, &["Makefile", "makefile", "GNUmakefile"])?;
    let tasks = parse_make_targets(&read(dir, file)?)
        .into_iter()
        .map(|(name, desc)| Task::new(&name, "make", &["make", &name]).with_description(desc))
        .collect();
    found(Ecosystem::Make, tasks)
}

/// Targets with an optional `## description` (common self-documenting convention).
pub fn parse_make_targets(text: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for line in text.lines() {
        if line.starts_with([' ', '\t', '#', '.']) || line.trim().is_empty() {
            continue;
        }
        let Some(colon) = line.find(':') else {
            continue;
        };
        let (head, rest) = line.split_at(colon);
        if rest.starts_with(":=") || rest.starts_with("::=") || head.contains(['=', '$', '%']) {
            continue;
        }
        let desc = rest.split_once("##").map(|(_, d)| d.trim().to_owned());
        for name in head.split_whitespace() {
            if !out.iter().any(|(n, _)| n == name) {
                out.push((name.to_owned(), desc.clone()));
            }
        }
    }
    out
}

fn just(dir: &Path, _: &Path) -> Option<Detection> {
    let file = first_existing(dir, &["justfile", "Justfile", ".justfile"])?;
    let text = read(dir, file)?;
    let mut params = just_params(&text);
    let tasks = parse_just_recipes(&text)
        .into_iter()
        .map(|(name, desc)| {
            let mut t = Task::new(&name, "just", &["just", &name]).with_description(desc);
            t.params = params.remove(&name).unwrap_or_default();
            t
        })
        .collect();
    found(Ecosystem::Just, tasks)
}

/// Words of a recipe header, keeping quoted strings and parenthesised expressions whole.
fn header_words(head: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quote, mut parens) = (None::<char>, 0usize);
    for c in head.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
                continue;
            }
            None if c == '"' || c == '\'' || c == '`' => quote = Some(c),
            None if c == '(' => parens += 1,
            None if c == ')' => parens = parens.saturating_sub(1),
            None if c.is_whitespace() && parens == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                continue;
            }
            None => {}
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The colon that ends a recipe header: outside quotes and parentheses, not `:=`.
fn recipe_colon(line: &str) -> Option<usize> {
    let (mut quote, mut parens) = (None::<char>, 0usize);
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' | '`' => quote = Some(c),
                '(' => parens += 1,
                ')' => parens = parens.saturating_sub(1),
                ':' if parens == 0 => {
                    return (!line[i..].starts_with(":=")).then_some(i);
                }
                _ => {}
            },
        }
    }
    None
}

/// Recipe parameters without a default, per recipe: `deploy env target="x" +files *rest:`
/// needs `env` and at least one of `files`.
pub fn just_params(text: &str) -> HashMap<String, Vec<Param>> {
    const SKIP: &[&str] = &["set ", "alias ", "export ", "import ", "mod "];
    let mut out = HashMap::new();
    for line in text.lines() {
        if line.is_empty()
            || line.starts_with([' ', '\t', '#', '['])
            || SKIP.iter().any(|s| line.starts_with(s))
        {
            continue;
        }
        let Some(colon) = recipe_colon(line) else {
            continue;
        };
        let words = header_words(line[..colon].trim_start_matches('@'));
        let Some((name, rest)) = words.split_first() else {
            continue;
        };
        let mut params: Vec<Param> = Vec::new();
        for w in rest {
            if w.starts_with('=') {
                params.pop(); // `name = "default"`: the previous word has a default
                continue;
            }
            if w.contains('=') || w.starts_with(['"', '\'', '`', '(', '*']) {
                continue;
            }
            let (form, w) = match w.strip_prefix('+') {
                Some(v) => (ParamForm::Variadic, v),
                None => (ParamForm::Positional, w.as_str()),
            };
            params.push(Param::new(w.trim_start_matches('$'), form));
        }
        if !params.is_empty() {
            out.insert(name.clone(), params);
        }
    }
    out
}

/// `requires: vars` of each task in a Taskfile: `vars: [ENV]`, or a block of `- ENV` /
/// `- name: ENV` items. A minimal scan, like `parse_taskfile_tasks`.
pub fn taskfile_requires(text: &str) -> HashMap<String, Vec<Param>> {
    let mut out: HashMap<String, Vec<Param>> = HashMap::new();
    let (mut in_tasks, mut level) = (false, None::<usize>);
    let (mut task, mut req, mut vars) = (None::<String>, None::<usize>, None::<usize>);
    let plain = |s: &str| {
        let s = s.trim().trim_matches(['"', '\'']);
        (!s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')).then(|| s.to_owned())
    };
    for line in text.lines() {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let depth = line.len() - t.len();
        if depth == 0 {
            in_tasks = t.starts_with("tasks:");
            (level, task, req, vars) = (None, None, None, None);
            continue;
        }
        if !in_tasks {
            continue;
        }
        let lv = *level.get_or_insert(depth);
        if depth <= lv {
            task = t
                .strip_suffix(':')
                .or_else(|| t.split_once(": ").map(|(n, _)| n))
                .map(|n| n.trim_matches(['"', '\'']).to_owned());
            (req, vars) = (None, None);
            continue;
        }
        let mut found: Vec<String> = Vec::new();
        if vars.is_some_and(|v| depth > v) {
            if let Some(item) = t.strip_prefix("- ") {
                let item = item.trim();
                found.extend(plain(item.strip_prefix("name:").unwrap_or(item)));
            }
        } else if req.is_some_and(|r| depth > r) {
            vars = None;
            if let Some(rest) = t.strip_prefix("vars:") {
                match rest.trim().strip_prefix('[') {
                    Some(flow) => {
                        found.extend(flow.trim_end_matches(']').split(',').filter_map(plain));
                    }
                    None => vars = Some(depth),
                }
            }
        } else {
            (req, vars) = (t.starts_with("requires:").then_some(depth), None);
        }
        if let (Some(name), false) = (&task, found.is_empty()) {
            let list = out.entry(name.clone()).or_default();
            for f in found {
                if !list.iter().any(|p| p.name == f) {
                    list.push(Param::new(f, ParamForm::Variable));
                }
            }
        }
    }
    out
}

/// A script that is only a command prefix: a workspace fan-out (`pnpm -r`, `pnpm --filter web`,
/// `yarn workspace`, `turbo run`, `lerna exec`) or a runner with flags and no command (`npx`,
/// `pnpm exec`, `npm run`), or one ending with a flag that takes a value (`--filter`). Anything
/// with shell operators is left alone: kiwa never guesses what a shell line needs.
pub fn script_params(cmd: &str) -> Vec<Param> {
    const VALUE_FLAGS: &[(&str, &str)] = &[
        ("--filter", "filter"),
        ("-F", "filter"),
        ("--workspace", "workspace"),
        ("-w", "workspace"),
        ("--scope", "scope"),
        ("--project", "project"),
        ("--dir", "dir"),
        ("-C", "dir"),
    ];
    const NEEDS: &[(&[&str], &str)] = &[
        (&["npx"], "command"),
        (&["bunx"], "command"),
        (&["pnpx"], "command"),
        (&["pnpm"], "command"),
        (&["pnpm", "exec"], "command"),
        (&["pnpm", "dlx"], "command"),
        (&["npm", "exec"], "command"),
        (&["npm", "run"], "script"),
        (&["pnpm", "run"], "script"),
        (&["yarn", "run"], "script"),
        (&["bun", "run"], "script"),
        (&["yarn", "workspaces", "foreach"], "command"),
        (&["yarn", "workspaces", "run"], "script"),
        (&["turbo", "run"], "task"),
        (&["lerna", "run"], "script"),
        (&["lerna", "exec"], "command"),
    ];
    if ["&&", "||", "|", ";", ">", "<", "$(", "`"]
        .iter()
        .any(|op| cmd.contains(op))
    {
        return Vec::new();
    }
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let mut core: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if let Some((_, name)) = VALUE_FLAGS.iter().find(|(f, _)| *f == w) {
            if i + 1 == words.len() {
                out.push(Param::new(*name, ParamForm::Positional));
            }
            i += 2;
            continue;
        }
        if !w.starts_with('-') {
            core.push(w);
        }
        i += 1;
    }
    if core == ["yarn", "workspace"] {
        out.push(Param::new("workspace", ParamForm::Positional));
        out.push(Param::new("command", ParamForm::Variadic));
    } else if let Some((_, what)) = NEEDS.iter().find(|(p, _)| *p == core.as_slice()) {
        out.push(Param::new(*what, ParamForm::Variadic));
    }
    out
}

/// Public recipes with their preceding `# doc comment`.
pub fn parse_just_recipes(text: &str) -> Vec<(String, Option<String>)> {
    const SKIP: &[&str] = &["set ", "alias ", "export ", "import ", "mod ", "["];
    let mut out = Vec::new();
    let mut doc: Option<String> = None;
    for line in text.lines() {
        if let Some(c) = line.strip_prefix('#') {
            if !c.starts_with('!') {
                doc = Some(c.trim().to_owned());
            }
            continue;
        }
        if line.is_empty()
            || line.starts_with([' ', '\t'])
            || SKIP.iter().any(|s| line.starts_with(s))
        {
            if !line.starts_with('[') {
                doc = None;
            }
            continue;
        }
        if let Some(colon) = line.find(':') {
            if line[colon..].starts_with(":=") {
                doc = None;
                continue;
            }
            let head = line[..colon].trim_start_matches('@');
            if let Some(name) = head.split_whitespace().next()
                && !name.starts_with('_')
            {
                out.push((name.to_owned(), doc.take()));
            }
        }
        doc = None;
    }
    out
}

fn taskfile(dir: &Path, _: &Path) -> Option<Detection> {
    let file = first_existing(
        dir,
        &[
            "Taskfile.yml",
            "Taskfile.yaml",
            "taskfile.yml",
            "taskfile.yaml",
        ],
    )?;
    let text = read(dir, file)?;
    let mut requires = taskfile_requires(&text);
    let tasks = parse_taskfile_tasks(&text)
        .into_iter()
        .map(|(name, desc)| {
            let mut t = Task::new(&name, "task", &["task", &name]).with_description(desc);
            t.params = requires.remove(&name).unwrap_or_default();
            t
        })
        .collect();
    found(Ecosystem::Task, tasks)
}

/// Minimal YAML scan of the top-level `tasks:` map (avoids a YAML dependency).
pub fn parse_taskfile_tasks(text: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut in_tasks = false;
    let mut indent: Option<usize> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let depth = line.len() - trimmed.len();
        if depth == 0 {
            in_tasks = trimmed.starts_with("tasks:");
            indent = None;
            continue;
        }
        if !in_tasks {
            continue;
        }
        let level = *indent.get_or_insert(depth);
        if depth == level {
            if let Some(name) = trimmed
                .strip_suffix(':')
                .or_else(|| trimmed.split_once(": ").map(|(n, _)| n))
            {
                out.push((name.trim_matches(['"', '\'']).to_owned(), None));
            }
        } else if depth > level
            && let (Some(d), Some(last)) = (trimmed.strip_prefix("desc:"), out.last_mut())
        {
            last.1 = Some(d.trim().trim_matches(['"', '\'']).to_owned());
        }
    }
    out
}

fn mise(dir: &Path, _: &Path) -> Option<Detection> {
    let file = first_existing(dir, &["mise.toml", ".mise.toml"]);
    let task_dirs = ["mise-tasks", ".mise/tasks"];
    if file.is_none() && !task_dirs.iter().any(|d| dir.join(d).is_dir()) {
        return None;
    }
    let mut tasks = Vec::new();
    if let Some(cfg) = file
        .and_then(|f| read(dir, f))
        .and_then(|t| t.parse::<toml::Table>().ok())
        && let Some(t) = cfg.get("tasks").and_then(|t| t.as_table())
    {
        for (name, def) in t {
            let desc = def
                .get("description")
                .and_then(|d| d.as_str())
                .map(str::to_owned);
            tasks.push(Task::new(name, "mise", &["mise", "run", name]).with_description(desc));
        }
    }
    for d in task_dirs {
        if let Ok(entries) = fs::read_dir(dir.join(d)) {
            for e in entries.flatten().filter(|e| e.path().is_file()) {
                let name = e
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                tasks.push(Task::new(&name, "mise", &["mise", "run", &name]));
            }
        }
    }
    // A bare mise.toml with only [tools] is still useful information, keep the project.
    found(Ecosystem::Mise, tasks)
}

// ---------------------------------------------------------------- JVM / .NET / PHP / Ruby

fn wrapper(dir: &Path, unix: &str, windows: &str, fallback: &str) -> String {
    let w = if cfg!(windows) { windows } else { unix };
    let p = dir.join(w);
    if p.is_file() {
        p.to_string_lossy().into_owned()
    } else {
        fallback.to_owned()
    }
}

fn gradle(dir: &Path, _: &Path) -> Option<Detection> {
    first_existing(
        dir,
        &[
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ],
    )?;
    let g = wrapper(dir, "gradlew", "gradlew.bat", "gradle");
    let tasks = ["build", "test", "clean", "tasks"]
        .iter()
        .map(|t| Task::new(*t, "gradle", &[&g, t]))
        .collect();
    found(Ecosystem::Gradle, tasks)
}

fn maven(dir: &Path, _: &Path) -> Option<Detection> {
    dir.join("pom.xml").is_file().then_some(())?;
    let m = wrapper(dir, "mvnw", "mvnw.cmd", "mvn");
    let tasks = ["package", "test", "verify", "clean"]
        .iter()
        .map(|t| Task::new(*t, "maven", &[&m, t]))
        .collect();
    found(Ecosystem::Maven, tasks)
}

fn dotnet(dir: &Path, _: &Path) -> Option<Detection> {
    let mut has_sln = false;
    let mut has_proj = false;
    for e in fs::read_dir(dir).ok()?.flatten() {
        let n = e.file_name().to_string_lossy().to_lowercase();
        has_sln |= n.ends_with(".sln") || n.ends_with(".slnx");
        has_proj |= n.ends_with(".csproj") || n.ends_with(".fsproj") || n.ends_with(".vbproj");
    }
    if !has_sln && !has_proj {
        return None;
    }
    let mut tasks: Vec<Task> = ["build", "test", "restore"]
        .iter()
        .map(|t| Task::new(*t, "dotnet", &["dotnet", t]))
        .collect();
    if has_proj {
        tasks.push(Task::new("run", "dotnet", &["dotnet", "run"]));
    }
    found(Ecosystem::Dotnet, tasks)
}

fn php(dir: &Path, _: &Path) -> Option<Detection> {
    let c: Value = match serde_json::from_str(&read(dir, "composer.json")?) {
        Ok(v) => v,
        Err(e) => {
            return broken(
                Ecosystem::Php,
                "composer.json",
                format!("invalid JSON ({e})"),
            );
        }
    };
    let descs = c.get("scripts-descriptions").and_then(Value::as_object);
    let mut tasks = vec![Task::new("install", "composer", &["composer", "install"])];
    if let Some(scripts) = c.get("scripts").and_then(Value::as_object) {
        for name in scripts.keys() {
            let d = descs
                .and_then(|d| d.get(name))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let mut t =
                Task::new(name, "composer", &["composer", "run-script", name]).with_description(d);
            // getcomposer.org/doc/articles/scripts.md: extra arguments go after `--`.
            t.separator = Separator::Always;
            tasks.push(t);
        }
    }
    found(Ecosystem::Php, tasks)
}

fn ruby(dir: &Path, _: &Path) -> Option<Detection> {
    let gemfile = dir.join("Gemfile").is_file();
    let rakefile = first_existing(dir, &["Rakefile", "rakefile", "Rakefile.rb"]);
    if !gemfile && rakefile.is_none() {
        return None;
    }
    let mut tasks = Vec::new();
    if gemfile {
        tasks.push(Task::new("install", "bundle", &["bundle", "install"]));
    }
    if let Some(text) = rakefile.and_then(|f| read(dir, f)) {
        for name in parse_rake_tasks(&text) {
            let argv: Vec<&str> = if gemfile {
                vec!["bundle", "exec", "rake", &name]
            } else {
                vec!["rake", &name]
            };
            tasks.push(Task::new(&name, "rake", &argv));
        }
    }
    found(Ecosystem::Ruby, tasks)
}

/// `task :name`, `task name: [...]`, `task "name"`.
pub fn parse_rake_tasks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        let Some(rest) = line
            .strip_prefix("task ")
            .or_else(|| line.strip_prefix("task("))
        else {
            continue;
        };
        let name: String = rest
            .trim_start_matches([':', '"', '\'', '('])
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn compose(dir: &Path, _: &Path) -> Option<Detection> {
    let file = first_existing(
        dir,
        &[
            "compose.yaml",
            "compose.yml",
            "docker-compose.yml",
            "docker-compose.yaml",
        ],
    )?;
    let mut tasks: Vec<Task> = [
        ("up", &["docker", "compose", "up"][..]),
        ("down", &["docker", "compose", "down"]),
        ("logs", &["docker", "compose", "logs", "-f"]),
        ("ps", &["docker", "compose", "ps"]),
    ]
    .iter()
    .map(|(n, argv)| Task::new(format!("compose:{n}"), "docker", argv))
    .collect();
    let text = read(dir, file).unwrap_or_default();
    for (service, profiles) in parse_compose_services(&text) {
        // A service behind a profile only starts when that profile is active.
        let mut base = vec!["docker".to_owned(), "compose".to_owned()];
        if let Some(p) = profiles.first() {
            base.extend(["--profile".to_owned(), p.clone()]);
        }
        for (verb, args) in [
            ("up", &["up"][..]),
            ("stop", &["stop"]),
            ("restart", &["restart"]),
            ("logs", &["logs", "-f"]),
            ("sh", &["exec"]),
        ] {
            let mut argv = base.clone();
            argv.extend(args.iter().map(|a| (*a).to_owned()));
            argv.push(service.clone());
            if verb == "sh" {
                argv.push("sh".into());
            }
            let t = Task::from_argv(format!("{service}:{verb}"), "docker", argv);
            tasks.push(t.with_description(Some(match verb {
                "up" => format!("start the `{service}` service"),
                "stop" => format!("stop the `{service}` service"),
                "restart" => format!("restart the `{service}` service"),
                "logs" => format!("follow the logs of `{service}`"),
                _ => format!("a shell inside `{service}`"),
            })));
        }
    }
    found(Ecosystem::Compose, tasks)
}

/// Folders whose files are a project's own commands.
const SCRIPT_DIRS: &[&str] = &["scripts", "bin", "tools"];

/// Script files in `scripts/`, `bin/` and `tools/` (not recursive). A file counts when it starts
/// with a shebang or has a script extension, so compiled programs in a `bin/` never do.
fn scripts(dir: &Path) -> Option<Detection> {
    let mut tasks = Vec::new();
    for folder in SCRIPT_DIRS {
        let Ok(rd) = fs::read_dir(dir.join(folder)) else {
            continue;
        };
        let mut files: Vec<String> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.'))
            .collect();
        files.sort();
        for name in files {
            let rel = format!("{folder}/{name}");
            let head = read_head(&dir.join(&rel));
            if let Some(argv) = script_argv(&rel, &head) {
                let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
                let desc = format!("{rel}{}", script_summary(&head));
                tasks.push(Task::from_argv(stem, folder, argv).with_description(Some(desc)));
            }
        }
    }
    if tasks.is_empty() {
        return None;
    }
    let mut d = found(Ecosystem::Scripts, tasks)?;
    d.from = SCRIPT_DIRS
        .iter()
        .filter(|f| dir.join(f).is_dir())
        .map(|f| format!("{f}/"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(d)
}

/// The first bytes of a file, as text (enough for a shebang and a comment line).
fn read_head(path: &Path) -> String {
    use std::io::Read;
    let mut buf = [0u8; 512];
    let n = fs::File::open(path)
        .and_then(|mut f| f.read(&mut buf))
        .unwrap_or(0);
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// ` · <first comment>` after the shebang, used as the task's description.
fn script_summary(head: &str) -> String {
    head.lines()
        .skip_while(|l| l.starts_with("#!"))
        .map(str::trim)
        .find(|l| !l.is_empty())
        .and_then(|l| l.strip_prefix('#').or_else(|| l.strip_prefix("//")))
        .map(|c| c.trim_start_matches(['#', ' ']).trim())
        .filter(|c| !c.is_empty() && !c.starts_with("requires") && c.len() < 120)
        .map(|c| format!(" · {c}"))
        .unwrap_or_default()
}

/// How a script runs: the interpreter its shebang names (`#!/usr/bin/env python3 -u` →
/// `python3 -u`), else the one its extension implies. `None` for anything else.
pub fn script_argv(rel: &str, head: &str) -> Option<Vec<String>> {
    let path = rel.to_owned();
    if let Some(line) = head.lines().next().and_then(|l| l.strip_prefix("#!")) {
        let mut words: Vec<&str> = line.split_whitespace().collect();
        let first = *words.first()?;
        let prog = first.rsplit('/').next().unwrap_or(first);
        let mut argv: Vec<String> = Vec::new();
        if prog == "env" {
            words.remove(0);
            words.retain(|w| *w != "-S");
            argv.extend(words.iter().map(|w| (*w).to_owned()));
        } else {
            argv.push(prog.to_owned());
            argv.extend(words[1..].iter().map(|w| (*w).to_owned()));
        }
        if argv.is_empty() {
            return None;
        }
        argv.push(path);
        return Some(argv);
    }
    let ext = rel.rsplit_once('.').map(|(_, e)| e.to_lowercase())?;
    let python = if cfg!(windows) { "python" } else { "python3" };
    let interp: &[&str] = match ext.as_str() {
        "sh" => &["sh"],
        "bash" => &["bash"],
        "zsh" => &["zsh"],
        "ps1" => &["pwsh", "-NoProfile", "-File"],
        "py" => &[python],
        "rb" => &["ruby"],
        "js" | "mjs" | "cjs" => &["node"],
        "ts" => &["npx", "tsx"],
        "cmd" | "bat" if cfg!(windows) => &["cmd", "/c"],
        _ => return None,
    };
    let mut argv: Vec<String> = interp.iter().map(|s| (*s).to_owned()).collect();
    argv.push(if matches!(ext.as_str(), "cmd" | "bat") {
        path.replace('/', "\\")
    } else {
        path
    });
    Some(argv)
}

/// Services of a Compose file with their profiles: the keys of the top-level `services:` map,
/// and each one's `profiles: [a, b]` or block list. A minimal scan, like the Taskfile one.
pub fn parse_compose_services(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let (mut inside, mut level) = (false, None::<usize>);
    let mut in_profiles: Option<usize> = None;
    let clean = |s: &str| s.trim().trim_matches(['"', '\'']).to_owned();
    for line in text.lines() {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let depth = line.len() - t.len();
        if depth == 0 {
            inside = t.starts_with("services:");
            level = None;
            continue;
        }
        if !inside {
            continue;
        }
        let lv = *level.get_or_insert(depth);
        if depth == lv {
            in_profiles = None;
            if let Some(name) = t
                .strip_suffix(':')
                .or_else(|| t.split_once(':').map(|(n, _)| n))
            {
                out.push((clean(name), Vec::new()));
            }
            continue;
        }
        let Some(last) = out.last_mut() else {
            continue;
        };
        if let Some(d) = in_profiles {
            if depth > d {
                if let Some(item) = t.strip_prefix("- ") {
                    last.1.push(clean(item));
                }
                continue;
            }
            in_profiles = None;
        }
        if let Some(rest) = t.strip_prefix("profiles:") {
            match rest.trim().strip_prefix('[') {
                Some(flow) => last.1.extend(
                    flow.trim_end_matches(']')
                        .split(',')
                        .map(clean)
                        .filter(|s| !s.is_empty()),
                ),
                None => in_profiles = Some(depth),
            }
        }
    }
    out
}

/// `devcontainer.json` (or `.devcontainer.json`, or one per folder under `.devcontainer/`):
/// `devcontainer up`, and each lifecycle command. `initializeCommand` runs on the host; the
/// others run inside the container through `devcontainer exec`.
fn devcontainer(dir: &Path, _: &Path) -> Option<Detection> {
    let mut files: Vec<String> = [".devcontainer/devcontainer.json", ".devcontainer.json"]
        .iter()
        .filter(|f| dir.join(f).is_file())
        .map(|f| (*f).to_owned())
        .collect();
    if let Ok(rd) = fs::read_dir(dir.join(".devcontainer")) {
        let mut named: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().join("devcontainer.json").is_file())
            .map(|e| {
                format!(
                    ".devcontainer/{}/devcontainer.json",
                    e.file_name().to_string_lossy()
                )
            })
            .collect();
        named.sort();
        files.extend(named);
    }
    let first = files.first()?.clone();
    let mut tasks = Vec::new();
    let mut warnings = Vec::new();
    for file in &files {
        // A named configuration (`.devcontainer/python/devcontainer.json`) prefixes its tasks.
        let (scope, config): (String, Vec<&str>) = match file.split('/').collect::<Vec<_>>()[..] {
            [_, name, _] => (format!("{name}:"), vec!["--config", file]),
            _ => (String::new(), vec![]),
        };
        let mut ws = vec!["--workspace-folder", "."];
        ws.extend(config.iter().copied());
        let value: Value = match read(dir, file).map(|t| serde_json::from_str(&jsonc(&t))) {
            Some(Ok(v)) => v,
            Some(Err(e)) => {
                warnings.push(format!("{file}: invalid JSON ({e})"));
                continue;
            }
            None => continue,
        };
        let mut up = vec!["devcontainer", "up"];
        up.extend(ws.iter().copied());
        tasks.push(
            Task::new(format!("devcontainer:{scope}up"), "devcontainer", &up)
                .with_description(Some("create and start the dev container".into())),
        );
        let mut exec = vec!["devcontainer", "exec"];
        exec.extend(ws.iter().copied());
        for key in [
            "initializeCommand",
            "onCreateCommand",
            "updateContentCommand",
            "postCreateCommand",
            "postStartCommand",
            "postAttachCommand",
        ] {
            let Some(v) = value.get(key) else { continue };
            let short = key.trim_end_matches("Command");
            let prefix: &[&str] = if key == "initializeCommand" {
                &[]
            } else {
                &exec
            };
            let name = format!("devcontainer:{scope}{short}");
            match v {
                Value::Object(parallel) => {
                    for (sub, cmd) in parallel {
                        tasks.extend(lifecycle_task(&format!("{name}:{sub}"), prefix, cmd));
                    }
                }
                cmd => tasks.extend(lifecycle_task(&name, prefix, cmd)),
            }
        }
    }
    Some(Detection {
        ecosystem: Ecosystem::Devcontainer,
        tasks,
        warnings,
        hints: Vec::new(),
        from: first,
    })
}

/// One lifecycle command: a string is a shell line, an array an argv (the spec's two forms).
fn lifecycle_task(name: &str, prefix: &[&str], cmd: &Value) -> Option<Task> {
    match cmd {
        Value::String(line) => Some(Task::from_line_in(name, "devcontainer", prefix, line)),
        Value::Array(words) => {
            let mut argv: Vec<String> = prefix.iter().map(|s| (*s).to_owned()).collect();
            argv.extend(words.iter().filter_map(|w| w.as_str().map(str::to_owned)));
            (argv.len() > prefix.len()).then(|| Task::from_argv(name, "devcontainer", argv))
        }
        _ => None,
    }
}

/// JSON with comments and trailing commas (JSONC, as `devcontainer.json` and VS Code files are
/// written) turned into strict JSON. Strings are copied untouched.
pub fn jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                out.extend(chars.next());
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }
    // Trailing commas: a comma followed only by whitespace before `}` or `]`.
    let mut strict = String::with_capacity(out.len());
    let mut in_string = false;
    let mut escaped = false;
    let bytes: Vec<char> = out.chars().collect();
    for (i, &c) in bytes.iter().enumerate() {
        if in_string {
            in_string = c != '"' || escaped;
            escaped = c == '\\' && !escaped;
            strict.push(c);
            continue;
        }
        if c == '"' {
            in_string = true;
        }
        if c == ',' {
            let next = bytes[i + 1..].iter().find(|n| !n.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        strict.push(c);
    }
    strict
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_targets() {
        let t = parse_make_targets(
            "VAR := 1\n.PHONY: all\nall: build ## Build everything\n\tcc x\nbuild test:\n%.o: %.c\n",
        );
        let names: Vec<_> = t.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["all", "build", "test"]);
        assert_eq!(t[0].1.as_deref(), Some("Build everything"));
    }

    #[test]
    fn just_recipes() {
        let t = parse_just_recipes(
            "set shell := [\"bash\"]\nversion := \"1\"\n# Run the tests\ntest *args:\n  cargo test\n_private:\n  x\n@lint:\n  y\n",
        );
        assert_eq!(t.len(), 2);
        assert_eq!(t[0], ("test".into(), Some("Run the tests".into())));
        assert_eq!(t[1].0, "lint");
    }

    #[test]
    fn taskfile_tasks() {
        let t = parse_taskfile_tasks(
            "version: '3'\ntasks:\n  build:\n    desc: Build it\n    cmds: [go build]\n  test:\n    cmds: [go test]\nvars:\n  X: 1\n",
        );
        assert_eq!(
            t,
            [
                ("build".into(), Some("Build it".into())),
                ("test".into(), None)
            ]
        );
    }

    #[test]
    fn rake_tasks() {
        assert_eq!(
            parse_rake_tasks("task :spec\ntask default: [:spec]\n"),
            ["spec", "default"]
        );
    }

    fn node_in(json: &str, extra: &[(&str, &str)]) -> Detection {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("package.json"), json).unwrap();
        for (f, c) in extra {
            fs::write(d.path().join(f), c).unwrap();
        }
        node(d.path(), d.path()).unwrap()
    }

    #[test]
    fn node_bom_is_accepted() {
        let det = node_in("\u{feff}{\"scripts\":{\"dev\":\"vite\"}}", &[]);
        assert_eq!(det.tasks.len(), 1);
        assert!(det.warnings.is_empty());
    }

    #[test]
    fn node_invalid_json_is_reported_not_dropped() {
        let det = node_in("{ \"scripts\": { ", &[]);
        assert!(det.tasks.is_empty());
        assert!(det.warnings[0].contains("invalid JSON"));
        let det = node_in("{\"scripts\": [\"x\"]}", &[]);
        assert!(det.warnings[0].contains("must be an object"));
    }

    #[test]
    fn node_hooks_are_folded_except_yarn_berry() {
        let pkg = r#"{"scripts":{"prebuild":"a","build":"b","postbuild":"c","postinstall":"d"}}"#;
        let det = node_in(pkg, &[]);
        let names: Vec<_> = det.tasks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["build", "postinstall"]);
        assert_eq!(det.tasks[0].hooks, ["prebuild", "postbuild"]);

        let det = node_in(
            pkg,
            &[("yarn.lock", "# header\n\n__metadata:\n  version: 8\n")],
        );
        assert_eq!(
            det.tasks.len(),
            4,
            "Yarn 2+ does not run hooks: keep them runnable"
        );
    }

    #[test]
    fn yarn_generation_from_the_lockfile_header_then_yarnrc() {
        let pkg = r#"{"scripts":{"prebuild":"a","build":"b"}}"#;
        let classic = "# THIS IS AN AUTOGENERATED FILE.\n# yarn lockfile v1\n\n\"a@1\":\n";
        assert_eq!(
            node_in(pkg, &[("yarn.lock", classic)]).tasks.len(),
            1,
            "Yarn 1 folds"
        );
        let entries_only = "\"a@1\":\n  version \"1.0.0\"\n";
        assert_eq!(node_in(pkg, &[("yarn.lock", entries_only)]).tasks.len(), 1);
        let empty_with_rc = [("yarn.lock", ""), (".yarnrc.yml", "nodeLinker: pnp\n")];
        assert_eq!(
            node_in(pkg, &empty_with_rc).tasks.len(),
            2,
            "an empty lockfile defers to .yarnrc.yml"
        );
        let field = r#"{"packageManager":"yarn@1.22.22","scripts":{"prebuild":"a","build":"b"}}"#;
        assert_eq!(
            node_in(field, &empty_with_rc).tasks.len(),
            1,
            "the packageManager field wins"
        );
    }

    #[test]
    fn the_newest_lockfile_wins_and_leftovers_are_reported() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("package.json"), r#"{"scripts":{"x":"y"}}"#).unwrap();
        let old = fs::File::create(d.path().join("package-lock.json")).unwrap();
        old.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
        fs::write(d.path().join("pnpm-lock.yaml"), "").unwrap();
        let det = node(d.path(), d.path()).unwrap();
        assert_eq!(det.tasks[0].runner, "pnpm");
        assert_eq!(det.from, "pnpm-lock.yaml");
        assert!(
            det.warnings[0].contains("pnpm is used")
                && det.warnings[0].contains("package-lock.json"),
            "{:?}",
            det.warnings
        );
        let det = node_in(
            r#"{"scripts":{"x":"y"}}"#,
            &[("bun.lock", ""), ("bun.lockb", "")],
        );
        assert!(
            det.warnings.is_empty(),
            "two files of one manager are not a conflict"
        );
    }

    #[test]
    fn node_pm_from_shrinkwrap() {
        let det = node_in(r#"{"scripts":{"x":"y"}}"#, &[("npm-shrinkwrap.json", "{}")]);
        assert_eq!(det.tasks[0].runner, "npm");
    }

    #[test]
    fn node_pm_from_lockfile_in_parent() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("pnpm-lock.yaml"), "").unwrap();
        let pkg = root.path().join("packages/web");
        fs::create_dir_all(&pkg).unwrap();
        assert_eq!(
            node_package_manager(&pkg, root.path(), &Value::Null),
            "pnpm"
        );
        let v: Value = serde_json::json!({"packageManager": "bun@1.2.0"});
        assert_eq!(node_package_manager(&pkg, root.path(), &v), "bun");
    }
}
