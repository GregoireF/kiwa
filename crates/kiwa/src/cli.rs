use clap::builder::styling::{AnsiColor, Styles};
use clap::{Parser, Subcommand, ValueEnum};

/// Coloured --help (clap disables colours with NO_COLOR and when output is not a terminal).
const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Cyan.on_default().bold())
    .usage(AnsiColor::Cyan.on_default().bold())
    .literal(AnsiColor::Cyan.on_default())
    .placeholder(AnsiColor::BrightBlack.on_default());
use std::path::PathBuf;

/// Find any project. Run any task. Built for humans and AI agents.
///
/// Run it alone for the interactive picker, or with `<project> <task>` to run directly.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "kiwa",
    version,
    styles = STYLES,
    allow_external_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Workspace root to scan (repeatable). Overrides `roots` in the config.
    #[arg(short = 'w', long = "root", global = true, value_name = "DIR")]
    pub roots: Vec<PathBuf>,

    /// Maximum directory depth to scan below each root.
    #[arg(long, global = true, value_name = "N")]
    pub depth: Option<usize>,

    /// Machine-readable output (versioned `kiwa/v1` envelope on stdout).
    #[arg(long, global = true, env = "KIWA_JSON")]
    pub json: bool,

    /// Keyboard only in the picker, for this run.
    #[arg(long, global = true)]
    pub no_mouse: bool,

    /// Answer yes to confirmations: a shell line, a risky task, `reset`.
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// Use a named workspace from the config (`workspace "work" { roots … }`).
    #[arg(
        short = 'W',
        long = "workspace",
        global = true,
        value_name = "NAME",
        env = "KIWA_WORKSPACE"
    )]
    pub workspace: Option<String>,
}

/// The examples under `--help`, written with the name the program was started as.
pub fn after_help(me: &str) -> String {
    let rows = [
        ("", "fuzzy-pick a project + task"),
        (" web", "pick among the tasks of `web`"),
        (" web dev", "run task `dev` of project `web`"),
        (" run api test -- -v", "pass extra args after --"),
        (" list --json", "machine-readable inventory for agents"),
        (" last", "re-run the last task"),
        (" init", "choose the folders to scan"),
        (" doctor", "what kiwa sees on this machine"),
    ];
    let mut s = String::from("Examples:\n");
    for (args, what) in rows {
        let cmd = format!("{me}{args}");
        s.push_str(&format!("  {cmd:<24}{what}\n"));
    }
    s.push_str("\nEnvironment:\n  KIWA_CONFIG   config file path      KIWA_STATE  history file path\n  KIWA_JSON=1   same as --json         NO_COLOR    disable colours");
    s
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Interactive fuzzy picker (default when run in a terminal).
    Pick,
    /// List discovered projects.
    #[command(alias = "ls")]
    List {
        /// Also list every task.
        #[arg(short, long)]
        tasks: bool,
    },
    /// Show a project's details and tasks.
    Info { project: String },
    /// Run a task in a project.
    Run {
        project: String,
        task: Option<String>,
        /// Print what would run, without running it.
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Extra arguments passed to the task.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Re-run the most recent task.
    Last {
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// Print a project's directory (used by the `kcd` shell function).
    Path { project: String },
    /// Choose the folders to scan and write the config (first-run setup).
    Init {
        /// Folder(s) to use, skipping the interactive questions.
        #[arg(long = "use", value_name = "DIR")]
        roots: Vec<PathBuf>,
        /// Accent colour: cyan, azure, indigo, violet, rose, amber, steel, or none.
        #[arg(long)]
        accent: Option<String>,
    },
    /// Shortcuts: `kw <alias>` runs a project task with preset arguments.
    Alias {
        #[command(subcommand)]
        action: AliasAction,
    },
    /// Inspect or change the configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Show (or clear) the run history used for ranking.
    History {
        #[arg(long)]
        clear: bool,
    },
    /// Print shell code: `kcd` (jump to a project), completions and alias commands.
    ShellInit {
        shell: Shell,
        /// Also open the picker under this command name (refused if it would shadow a command).
        #[arg(long, value_name = "CMD")]
        name: Option<String>,
    },
    /// Print the completion script for a shell.
    Completions { shell: Shell },
    /// What a project is and how to start it: description, licence, tasks, required versions.
    About {
        /// Project name or folder (default: the project in the current folder).
        project: Option<String>,
    },
    /// Get a project running: install dependencies, create .env, run its setup tasks.
    Setup {
        /// Project name or folder (default: the project in the current folder).
        project: Option<String>,
        /// Show the plan without running anything.
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// What kiwa sees here: terminal, colours, mouse, files, runners on PATH.
    Doctor {
        /// Check interactively what this terminal sends (clicks, motion, wheel, colours).
        #[arg(long)]
        probe: bool,
    },
    /// Delete the configuration, aliases and history (`--yes` skips the question).
    Reset,
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigAction {
    /// Print the config file location.
    Path,
    /// Print the effective configuration.
    Show,
    /// Print one setting (roots, depth, ignore, accent, mouse, hide, show-git-only, workspaces, aliases).
    Get { key: String },
    /// Open the config file in $VISUAL, $EDITOR or the system's default editor.
    Edit,
    /// Set a value: roots, depth, ignore, accent, mouse, hide, show-git-only. Comments are kept.
    Set {
        key: String,
        #[arg(required = true, num_args = 1..)]
        values: Vec<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum AliasAction {
    /// Add (or replace) an alias: `kw alias add t api test -- -v`.
    Add {
        name: String,
        project: String,
        task: String,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Remove an alias.
    #[command(alias = "rm")]
    Remove { name: String },
    /// List aliases.
    #[command(alias = "ls")]
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    #[value(alias = "pwsh")]
    Powershell,
    Elvish,
    #[value(alias = "nu")]
    Nushell,
}
