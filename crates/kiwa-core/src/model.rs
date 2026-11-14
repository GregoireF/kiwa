use serde::Serialize;
use std::path::PathBuf;

/// An ecosystem or task runner kiwa knows how to detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ecosystem {
    Node,
    Deno,
    Rust,
    Go,
    Python,
    Make,
    Just,
    Task,
    Mise,
    Gradle,
    Maven,
    Dotnet,
    Php,
    Ruby,
    Compose,
    /// `devcontainer.json`: the dev container and its lifecycle commands.
    Devcontainer,
    /// Script files in `scripts/`, `bin/` or `tools/` of a project.
    Scripts,
    /// `Procfile` processes.
    Procfile,
    /// `.vscode/tasks.json`.
    Vscode,
    /// CI jobs reproduced locally (GitHub Actions, GitLab CI).
    Ci,
    /// Nx project targets (`project.json`).
    Nx,
    /// Shell steps of the README's quick-start sections.
    Readme,
    /// Turborepo tasks (`turbo.json`).
    Turbo,
    /// A git repository with no recognised manifest: listed, without tasks.
    Git,
}

impl Ecosystem {
    pub fn label(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Deno => "deno",
            Self::Rust => "rust",
            Self::Go => "go",
            Self::Python => "python",
            Self::Make => "make",
            Self::Just => "just",
            Self::Task => "task",
            Self::Mise => "mise",
            Self::Gradle => "gradle",
            Self::Maven => "maven",
            Self::Dotnet => "dotnet",
            Self::Php => "php",
            Self::Ruby => "ruby",
            Self::Compose => "compose",
            Self::Devcontainer => "devcontainer",
            Self::Scripts => "scripts",
            Self::Procfile => "procfile",
            Self::Vscode => "vscode",
            Self::Ci => "ci",
            Self::Nx => "nx",
            Self::Readme => "readme",
            Self::Turbo => "turbo",
            Self::Git => "git",
        }
    }
}

/// A runnable task: a fully resolved argv, executed in the project directory.
#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub name: String,
    /// The tool that runs it (`npm`, `pnpm`, `cargo`, `make`, `just`, ...).
    pub runner: String,
    /// Program followed by its arguments. Never passed through a shell.
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Lifecycle hooks the runner executes around this task (`prebuild`, `postbuild`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<String>,
    /// Input the task cannot run without: it fails, or does nothing useful, when none is given.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Param>,
    /// The command is a shell line run as `sh -c <line>`: shown and confirmed before it runs.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub shell: bool,
    /// The name looks destructive (`deploy`, `drop`, `…:prod`): confirmed before it runs.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub risky: bool,
    /// A step suggested by prose (a README code block), not declared by a tool: confirmed
    /// before it runs.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub suggested: bool,
    /// Where the dev server this task starts will listen (`http://localhost:5173`), worked out
    /// from its command and files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Whether forwarded arguments need a leading `--` for this runner.
    #[serde(skip)]
    pub separator: Separator,
}

/// Task names that ask for confirmation unless the config's `risky` list replaces them.
pub const RISKY: &[&str] = &[
    "deploy",
    "publish",
    "release",
    "drop",
    "prune",
    "destroy",
    "reset",
    "purge",
    "wipe",
    "nuke",
    "teardown",
    "prod",
    "production",
];

/// Whether a task name is risky: one of its words (split on `:`, `-`, `_`, `.`, `/` and spaces)
/// is in `words`, case-insensitively. `db:reset`, `deploy-staging` and `migrate:prod` are.
pub fn is_risky(name: &str, words: &[&str]) -> bool {
    name.split([':', '-', '_', '.', '/', ' '])
        .any(|w| words.iter().any(|r| r.eq_ignore_ascii_case(w)))
}

/// One required input of a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Param {
    /// `env` (a just parameter), `ENV` (a Taskfile variable), `command`, `--filter`.
    pub name: String,
    pub form: ParamForm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParamForm {
    /// A word after the task: `just deploy prod`.
    Positional,
    /// One or more words: just's `+files`, or the command a script prefix is missing.
    Variadic,
    /// `NAME=value` after the task: `task deploy ENV=prod`.
    Variable,
}

impl Param {
    pub fn new(name: impl Into<String>, form: ParamForm) -> Self {
        Self {
            name: name.into(),
            form,
        }
    }

    pub fn usage(&self) -> String {
        match self.form {
            ParamForm::Positional => format!("<{}>", self.name),
            ParamForm::Variadic => format!("<{}>…", self.name),
            ParamForm::Variable => format!("{}=…", self.name),
        }
    }
}

/// How extra arguments reach the task. Measured per runner (see docs/explanation/args-forwarding.md):
/// npm and Yarn 1 need a leading `--`; pnpm 9, Yarn 2+ and Bun must not get one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Separator {
    #[default]
    Never,
    Always,
}

impl Task {
    pub fn new(name: impl Into<String>, runner: &str, argv: &[&str]) -> Self {
        Self::from_argv(name, runner, argv.iter().map(|s| (*s).to_owned()).collect())
    }

    pub fn from_argv(name: impl Into<String>, runner: &str, command: Vec<String>) -> Self {
        Self {
            name: name.into(),
            runner: runner.to_owned(),
            command,
            description: None,
            hooks: Vec::new(),
            params: Vec::new(),
            shell: false,
            risky: false,
            suggested: false,
            url: None,
            separator: Separator::Never,
        }
    }

    /// A task from a command line written in a file. A line a shell would run unchanged as an
    /// argv becomes that argv; any other runs as `sh -c <line>` and is marked `shell`.
    pub fn from_line(name: impl Into<String>, runner: &str, line: &str) -> Self {
        Self::from_line_in(name, runner, &[], line)
    }

    /// [`Task::from_line`] run through `prefix` (`devcontainer exec --workspace-folder .`): the
    /// argv, or `sh -c <line> sh`, comes after it.
    pub fn from_line_in(
        name: impl Into<String>,
        runner: &str,
        prefix: &[&str],
        line: &str,
    ) -> Self {
        let line = line.trim();
        let mut command: Vec<String> = prefix.iter().map(|s| (*s).to_owned()).collect();
        let shell = match crate::words::split(line) {
            Ok(argv) if !argv.is_empty() && !crate::words::needs_shell(line) => {
                command.extend(argv);
                false
            }
            _ => {
                command.extend(["sh".into(), "-c".into(), line.to_owned(), "sh".into()]);
                true
            }
        };
        let mut t = Self::from_argv(name, runner, command);
        t.shell = shell;
        t.description = Some(line.to_owned()).filter(|d| !d.is_empty());
        t
    }

    /// Where `sh -c <line>` starts in a `shell` task's command.
    pub fn shell_at(&self) -> Option<usize> {
        if !self.shell {
            return None;
        }
        self.command
            .windows(3)
            .position(|w| w[0] == "sh" && w[1] == "-c")
    }

    /// The shell line of a `shell` task.
    pub fn line(&self) -> Option<&str> {
        self.shell_at()
            .and_then(|i| self.command.get(i + 2))
            .map(String::as_str)
    }

    /// What to type for the missing input: `<env> <target>`, `ENV=…`, `<command>`.
    pub fn params_usage(&self) -> String {
        self.params
            .iter()
            .map(Param::usage)
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn with_description(mut self, d: Option<String>) -> Self {
        self.description = d.filter(|s| !s.trim().is_empty());
        self
    }

    /// `runner:name`, the unambiguous form accepted by `kw run`.
    pub fn qualified(&self) -> String {
        format!("{}:{}", self.runner, self.name)
    }
}

/// A directory recognised as a project by at least one detector.
#[derive(Debug, Clone, Serialize)]
pub struct Project {
    /// Stable identifier: path relative to its scan root, `/`-separated.
    pub id: String,
    /// Directory name.
    pub name: String,
    pub path: PathBuf,
    pub ecosystems: Vec<Ecosystem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The remote's default branch (`refs/remotes/origin/HEAD`), when the clone recorded it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    pub tasks: Vec<Task>,
    /// Manifest problems (invalid JSON, wrong types). Shown to the user, never fatal.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Helpful facts that are not errors (e.g. dependencies not installed yet).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
    /// Why each ecosystem was detected, and for JavaScript why that package manager
    /// (`pnpm-lock.yaml`, `packageManager: pnpm@9.1.0`, ...).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub detected_from: Vec<Source>,
    /// The monorepo that declares this project as a member, and the file that says so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_of: Option<Membership>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Membership {
    /// Id of the monorepo root project.
    pub root: String,
    pub path: PathBuf,
    /// `pnpm-workspace.yaml`, `package.json workspaces`, `Cargo.toml [workspace]`, …
    pub from: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Source {
    pub ecosystem: Ecosystem,
    pub from: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_match_serde_names() {
        use Ecosystem::*;
        for e in [
            Node,
            Deno,
            Rust,
            Go,
            Python,
            Make,
            Just,
            Task,
            Mise,
            Gradle,
            Maven,
            Dotnet,
            Php,
            Ruby,
            Compose,
            Devcontainer,
            Scripts,
            Procfile,
            Vscode,
            Ci,
            Nx,
            Readme,
            Turbo,
            Git,
        ] {
            let json = serde_json::to_string(&e).unwrap();
            assert_eq!(json, format!("\"{}\"", e.label()));
        }
    }

    #[test]
    fn lines_become_argv_or_confirmed_shell_tasks() {
        let t = super::Task::from_line("up", "devcontainer", "  npm ci  ");
        assert_eq!(
            (t.command.as_slice(), t.shell),
            (&["npm".to_owned(), "ci".into()][..], false)
        );
        assert_eq!(t.line(), None);
        let s = super::Task::from_line("ci", "github", "npm ci && npm test");
        assert!(s.shell);
        assert_eq!(s.command, ["sh", "-c", "npm ci && npm test", "sh"]);
        assert_eq!(s.line(), Some("npm ci && npm test"));
        assert_eq!(s.description.as_deref(), Some("npm ci && npm test"));
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["shell"], true);
        assert!(serde_json::to_value(&t).unwrap().get("shell").is_none());
    }

    #[test]
    fn risky_names() {
        for n in [
            "deploy",
            "db:reset",
            "deploy-staging",
            "migrate:prod",
            "Release",
            "docker.prune",
        ] {
            assert!(super::is_risky(n, super::RISKY), "{n}");
        }
        for n in [
            "dev",
            "test",
            "build",
            "reproduce",
            "deployment-docs",
            "preview",
        ] {
            assert!(!super::is_risky(n, super::RISKY), "{n}");
        }
        assert!(
            super::is_risky("ship", &["ship"]),
            "the list can be replaced"
        );
    }

    #[test]
    fn task_helpers() {
        let t = super::Task::new("build", "make", &["make", "build"])
            .with_description(Some("  ".into()));
        assert_eq!(t.qualified(), "make:build");
        assert!(t.description.is_none(), "blank descriptions are dropped");
        let json = serde_json::to_value(&t).unwrap();
        assert!(json.get("hooks").is_none() && json.get("separator").is_none());
    }
}
