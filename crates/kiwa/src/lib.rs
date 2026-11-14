//! kiwa: find any project, run any task. Built for humans and AI agents.
//!
//! The command is `kiwa`; `kw` is the same program under a shorter name, installed where it is
//! free (Debian's `kworkflow` ships `/usr/bin/kw`). Both binaries call [`main`]. Help, errors
//! and suggestions use the name that was typed (see [`me`]).

mod cli;
mod doctor;
mod exec;
mod init;
mod output;
mod shell;
mod theme;
mod tui;
use kiwa_core::words;

use clap::{CommandFactory, FromArgMatches};
use cli::{AliasAction, Cli, Command, ConfigAction};
use kiwa_core::config::{self, Config, Editor};
use kiwa_core::state::{self, State};
use kiwa_core::{DiscoverOptions, Project, discover, resolve, suggest};
use output::{Failure, Out};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The name this program was started as: `kw` when invoked as `kw`, `kiwa` otherwise.
pub fn me() -> &'static str {
    static ME: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    ME.get_or_init(|| {
        let stem = std::env::args_os()
            .next()
            .map(PathBuf::from)
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()));
        name_for(stem.as_deref())
    })
}

/// `kw` for a program file named `kw`, `kiwa` for anything else (a renamed copy included).
fn name_for(stem: Option<&str>) -> &'static str {
    if stem == Some("kw") { "kw" } else { "kiwa" }
}

/// Entry point of both binaries.
pub fn main() -> std::process::ExitCode {
    // The parent must survive Ctrl+C while a child task runs; the child gets the signal.
    let _ = ctrlc::set_handler(|| {});
    let matches = Cli::command()
        .name(me())
        .bin_name(me())
        .after_help(cli::after_help(me()))
        .get_matches();
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(c) => c,
        Err(e) => e.exit(),
    };
    let out = Out::new(cli.json);
    let code = match run(&cli, &out) {
        Ok(code) => code,
        Err(f) => {
            out.failure(&f);
            f.code
        }
    };
    // Return normally when possible so buffers are flushed and exit handlers run.
    // Windows status codes (e.g. 0xC000013A after Ctrl+C) do not fit in a u8: mirror them raw.
    match u8::try_from(code) {
        Ok(c) => std::process::ExitCode::from(c),
        Err(_) => std::process::exit(code),
    }
}

struct Scan {
    projects: Vec<Project>,
    elapsed: Duration,
    roots: Vec<PathBuf>,
}

/// Roots, depth and ignore list for this run: flags > named workspace > config > cwd.
fn scan_options(cli: &Cli, cfg: &Config) -> Result<DiscoverOptions, Failure> {
    let ws = match &cli.workspace {
        Some(name) => Some(cfg.workspace(name).ok_or_else(|| {
            Failure::new(
                3,
                "not-found",
                format!("no workspace named `{name}` in the config"),
            )
            .with_suggestions(cfg.workspaces.iter().map(|w| w.name.clone()).collect())
        })?),
        None => None,
    };
    let roots = if !cli.roots.is_empty() {
        cli.roots.clone()
    } else if let Some(w) = ws {
        w.roots.clone()
    } else if !cfg.roots.is_empty() {
        cfg.roots.clone()
    } else {
        vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
    };
    let mut ignore = cfg.ignore.clone();
    if let Some(w) = ws {
        ignore.extend(w.ignore.iter().cloned());
    }
    Ok(DiscoverOptions {
        roots,
        max_depth: cli
            .depth
            .or(ws.and_then(|w| w.depth))
            .or(cfg.depth)
            .unwrap_or(4),
        ignore,
    })
}

fn scan(cli: &Cli, cfg: &Config) -> Scan {
    // A wrong workspace name is reported by `checked_scan`; here it falls back to the defaults.
    scan_with(scan_options(cli, cfg).unwrap_or_default(), cfg)
}

/// Workspaces for the picker's switcher: `default` (the `-w` folders, the config's `roots`, or
/// the current folder), then the named ones, unless `-w` names the folders for this run.
fn spaces(cli: &Cli, cfg: &Config) -> Vec<tui::Space> {
    let abs = |r: &PathBuf| std::path::absolute(r).unwrap_or_else(|_| r.clone());
    let default = if !cli.roots.is_empty() {
        cli.roots.clone()
    } else if !cfg.roots.is_empty() {
        cfg.roots.clone()
    } else {
        vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
    };
    let mut v = vec![tui::Space {
        name: "default".into(),
        roots: default.iter().map(abs).collect(),
    }];
    if cli.roots.is_empty() {
        v.extend(cfg.workspaces.iter().map(|w| tui::Space {
            name: w.name.clone(),
            roots: w.roots.iter().map(abs).collect(),
        }));
    }
    v
}

/// The picker's scan. With named workspaces it covers all of them at once, so that switching
/// and `@name` filter without scanning again; depth is the deepest asked for.
fn picker_scan(cli: &Cli, cfg: &Config, spaces: &[tui::Space]) -> Scan {
    if spaces.len() < 2 {
        return scan(cli, cfg);
    }
    let base = scan_options(cli, cfg).unwrap_or_default();
    let mut roots: Vec<PathBuf> = Vec::new();
    for r in spaces.iter().flat_map(|s| &s.roots) {
        if !roots.contains(r) {
            roots.push(r.clone());
        }
    }
    let mut ignore = cfg.ignore.clone();
    for w in &cfg.workspaces {
        ignore.extend(w.ignore.iter().cloned());
    }
    let max_depth = cfg
        .workspaces
        .iter()
        .filter_map(|w| w.depth)
        .fold(base.max_depth, usize::max);
    scan_with(
        DiscoverOptions {
            roots,
            max_depth,
            ignore,
        },
        cfg,
    )
}

fn scan_with(opts: DiscoverOptions, cfg: &Config) -> Scan {
    let t = Instant::now();
    let mut projects = discover(&opts);
    // `hide` patterns and `show-git-only #false`.
    let risky = cfg.risky_words();
    for p in &mut projects {
        let id = p.id.clone();
        p.tasks.retain(|t| !cfg.hides(&id, &t.runner, &t.name));
        for t in &mut p.tasks {
            t.risky = kiwa_core::model::is_risky(&t.name, &risky);
        }
    }
    if cfg.show_git_only == Some(false) {
        projects.retain(|p| !(p.tasks.is_empty() && p.ecosystems == [kiwa_core::Ecosystem::Git]));
    }
    Scan {
        projects,
        elapsed: t.elapsed(),
        roots: opts.roots,
    }
}

/// In the picker, a project that is not set up yet (dependencies, `.env`) starts with an entry
/// that runs `kiwa setup` on it. Lists and JSON keep the project's own tasks only.
fn with_setup_entries(projects: &[Project]) -> Vec<Project> {
    let mut projects = projects.to_vec();
    if let Ok(exe) = std::env::current_exe() {
        for p in &mut projects {
            if !kiwa_core::setup::needed(p) {
                continue;
            }
            let plan = kiwa_core::setup::plan(p, false);
            let todo: Vec<&str> = plan.pending().map(|s| s.what.as_str()).collect();
            let name = if p.tasks.iter().any(|t| t.name == "setup") {
                "kiwa-setup"
            } else {
                "setup"
            };
            let argv = vec![
                exe.display().to_string(),
                "setup".to_owned(),
                p.path.display().to_string(),
            ];
            let t = kiwa_core::Task::from_argv(name, "kiwa", argv)
                .with_description(Some(format!("kiwa setup: {}", todo.join(", "))));
            p.tasks.insert(0, t);
        }
    }
    projects
}

fn internal(e: impl std::fmt::Display) -> Failure {
    Failure::new(1, "internal", e.to_string())
}

fn config_err(e: impl std::fmt::Display) -> Failure {
    Failure::new(5, "config", e.to_string())
}

fn interactive(cli: &Cli) -> bool {
    !cli.json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// Mouse capture is off with --no-mouse, `mouse #false`, or under mintty (Git Bash), whose
/// pty does not expose the Windows console API crossterm relies on (crossterm issue #580).
fn mouse(cli: &Cli, cfg: &Config) -> (bool, Option<&'static str>) {
    match mouse_reason(cli, cfg) {
        Some(r) if r.starts_with("mouse off in") => (false, Some(r)),
        Some(_) => (false, None),
        None => (true, None),
    }
}

/// Why the mouse is off, or `None` when it is on.
fn mouse_reason(cli: &Cli, cfg: &Config) -> Option<&'static str> {
    if cli.no_mouse {
        return Some("--no-mouse");
    }
    if cfg.mouse == Some(false) {
        return Some("`mouse #false` in the config");
    }
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let mintty = term_program == "mintty"
        || (std::env::var_os("MSYSTEM").is_some()
            && std::env::var_os("WT_SESSION").is_none()
            && term_program.is_empty());
    mintty.then_some("mouse off in mintty/Git Bash")
}

/// `kw doctor`: runs even when the config does not load, since that is what it diagnoses.
fn doctor(cli: &Cli, out: &Out, probe: bool) -> Result<i32, Failure> {
    let loaded = config::load().map_err(|e| e.to_string());
    let cfg = loaded.as_ref().cloned().unwrap_or_default();
    if probe {
        // The probe draws on stdout; JSON is printed there once it is over.
        if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
            return Err(Failure::new(
                2,
                "usage",
                format!(
                    "{0} doctor --probe needs a terminal; run `{0} doctor` for the static report",
                    me()
                ),
            ));
        }
        let th = theme::Theme::from_env(cfg.accent.as_deref());
        let Some(p) = doctor::probe(&th).map_err(internal)? else {
            return Ok(130);
        };
        if out.json {
            out.value("doctor-probe", &p);
        } else {
            println!("{}", doctor::render_probe(&p));
        }
        return Ok(0);
    }
    let reason = mouse_reason(cli, &cfg).map(str::to_owned);
    let r = doctor::report(
        (reason.is_none(), reason),
        loaded.as_ref().map_err(Clone::clone),
    );
    if out.json {
        out.value("doctor", &r);
    } else {
        println!("{}", doctor::render(&r));
    }
    Ok(0)
}

/// The picker on every workspace, showing `space` (a workspace name) or the one `-W` chose,
/// else `default`.
fn browse(cli: &Cli, cfg: &Config, space: Option<&str>) -> Result<i32, Failure> {
    let spaces = spaces(cli, cfg);
    let s = picker_scan(cli, cfg, &spaces);
    let want = space.or(cli.workspace.as_deref());
    let at = want
        .and_then(|n| spaces.iter().position(|s| s.name == n))
        .or(Some(0));
    pick_in(cli, &s, cfg, None, spaces, at)
}

/// A picker scoped to one project (`kw <project>`): the switcher offers `default` only.
fn pick(cli: &Cli, s: &Scan, cfg: &Config, scope: Option<PathBuf>) -> Result<i32, Failure> {
    let default = tui::Space {
        name: "default".into(),
        roots: s.roots.clone(),
    };
    pick_in(cli, s, cfg, scope, vec![default], Some(0))
}

fn pick_in(
    cli: &Cli,
    s: &Scan,
    cfg: &Config,
    scope: Option<PathBuf>,
    spaces: Vec<tui::Space>,
    space: Option<usize>,
) -> Result<i32, Failure> {
    let mut st = State::load();
    let (mouse, note) = mouse(cli, cfg);
    let opts = tui::Options {
        accent: cfg.accent.clone(),
        mouse,
        note,
        scope,
        roots: s.roots.clone(),
        spaces,
        space,
        home: dirs_home(),
    };
    let projects = with_setup_entries(&s.projects);
    match tui::run(&projects, &mut st, s.elapsed, opts) {
        Ok(tui::Outcome::Exit(code)) => Ok(code),
        Ok(tui::Outcome::Folders(_)) => {
            // `kw init` from the switcher: run the setup, then come back to the picker.
            let out = Out::new(false);
            match run_init(&out, cfg, false)? {
                Some(new_cfg) => browse(cli, &new_cfg, None),
                None => browse(cli, cfg, None),
            }
        }
        Ok(tui::Outcome::AddFolder(dir, _)) => {
            let name = add_workspace(cfg, &dir)?;
            let new_cfg = config::load().map_err(config_err)?;
            browse(cli, &new_cfg, Some(&name))
        }
        Err(e) => Err(internal(format!(
            "cannot open the interactive picker ({e}); use `{0} list` or `{0} <project> <task>`",
            me()
        ))),
    }
}

/// Save `dir` as a new named workspace, named after the folder (`work`, then `work-2`, …).
fn add_workspace(cfg: &Config, dir: &std::path::Path) -> Result<String, Failure> {
    let base: String = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "folder".into())
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let taken = |n: &str| n == "default" || n == "all" || cfg.workspace(n).is_some();
    let name = (1..)
        .map(|i| {
            if i == 1 {
                base.clone()
            } else {
                format!("{base}-{i}")
            }
        })
        .find(|n| !taken(n))
        .unwrap_or(base);
    let mut e = config::Editor::open_default().map_err(config_err)?;
    e.set_workspace(&name, &[dir.to_path_buf()]);
    e.save().map_err(config_err)?;
    Ok(name)
}

fn run(cli: &Cli, out: &Out) -> Result<i32, Failure> {
    if let Some(Command::Doctor { probe }) = &cli.command {
        return doctor(cli, out, *probe);
    }
    let cfg = config::load().map_err(config_err)?;
    if !cli.json {
        for w in &cfg.warnings {
            eprintln!("kiwa: warning: {w}");
        }
    }
    // Fail early on an unknown `-W` name, with suggestions.
    scan_options(cli, &cfg)?;

    match &cli.command {
        None | Some(Command::Pick) => {
            if !interactive(cli) {
                let s = scan(cli, &cfg);
                out.list(&s.projects, s.elapsed, &s.roots, false);
                return Ok(0);
            }
            // First run: nothing configured and nothing here → offer setup instead of an empty list.
            let first_run = cfg.path.as_ref().is_none_or(|p| !p.is_file()) && cli.roots.is_empty();
            if first_run && scan(cli, &cfg).projects.is_empty() {
                if let Some(cfg) = run_init(out, &cfg, true)? {
                    return browse(cli, &cfg, None);
                }
                return Ok(0);
            }
            browse(cli, &cfg, None)
        }
        Some(Command::List { tasks }) => {
            let s = scan(cli, &cfg);
            out.list(&s.projects, s.elapsed, &s.roots, *tasks);
            Ok(0)
        }
        Some(Command::Info { project }) => {
            let s = scan(cli, &cfg);
            let p = resolve::project(&s.projects, project)?;
            out.info(p);
            Ok(0)
        }
        Some(Command::Path { project }) => {
            let s = scan(cli, &cfg);
            let p = resolve::project(&s.projects, project)?;
            out.path(p);
            Ok(0)
        }
        Some(Command::Run {
            project,
            task,
            dry_run,
            args,
        }) => {
            let s = scan(cli, &cfg);
            if task.is_none() && interactive(cli) {
                let p = resolve::project(&s.projects, project)?;
                return pick(cli, &s, &cfg, Some(p.path.clone()));
            }
            run_task(
                out,
                &s.projects,
                project,
                task.as_deref(),
                args,
                *dry_run,
                cli.yes,
            )
        }
        Some(Command::Last { dry_run }) => {
            let st = State::load();
            let last = st
                .last()
                .ok_or_else(|| Failure::new(3, "not-found", "no task has been run yet".into()))?
                .clone();
            let opts = DiscoverOptions {
                roots: vec![last.path.clone()],
                max_depth: 0,
                ..Default::default()
            };
            // Through `scan_with`, so `hide` and `risky` apply to a re-run as to any run.
            let projects = scan_with(opts, &cfg).projects;
            let p = projects.first().ok_or_else(|| {
                Failure::new(
                    3,
                    "not-found",
                    format!("{} is no longer a project", last.path.display()),
                )
            })?;
            run_task(
                out,
                std::slice::from_ref(p),
                &p.id.clone(),
                Some(&last.task),
                &[],
                *dry_run,
                cli.yes,
            )
        }
        Some(Command::External(argv)) => {
            // Direct mode: `kw <alias|project> [task] [-- args]`
            let (head, mut extra) = match argv.iter().position(|a| a == "--") {
                Some(i) => (&argv[..i], argv[i + 1..].to_vec()),
                None => (&argv[..], Vec::new()),
            };
            // `kw app deploy --yes`: clap hands everything after `app` to direct mode, so the
            // confirmation flag is taken here too (before `--` only: after it, it is the task's).
            let given_yes = head.iter().any(|a| a == "--yes" || a == "-y");
            let head: Vec<String> = head
                .iter()
                .filter(|a| *a != "--yes" && *a != "-y")
                .cloned()
                .collect();
            let cli = &Cli {
                yes: cli.yes || given_yes,
                ..cli.clone()
            };
            let s = scan(cli, &cfg);
            if let Some(a) = cfg.aliases.iter().find(|a| a.name == head[0]) {
                if head.len() > 1 {
                    return Err(Failure::new(
                        2,
                        "usage",
                        format!("`{}` is an alias; put extra arguments after --", a.name),
                    ));
                }
                let mut args = a.args.clone();
                args.append(&mut extra);
                return run_task(
                    out,
                    &s.projects,
                    &a.project,
                    Some(&a.task),
                    &args,
                    false,
                    cli.yes,
                );
            }
            if head.len() > 2 {
                return Err(Failure::new(
                    2,
                    "usage",
                    format!(
                        "unexpected argument `{}`; pass task arguments after --: {} {} {} -- {}",
                        head[2],
                        me(),
                        head[0],
                        head[1],
                        head[2..].join(" ")
                    ),
                ));
            }
            if head.len() == 1 && interactive(cli) {
                // `kw <project>`: open the picker scoped to that project.
                let p = resolve::project(&s.projects, &head[0])?;
                return pick(cli, &s, &cfg, Some(p.path.clone()));
            }
            run_task(
                out,
                &s.projects,
                &head[0],
                head.get(1).map(String::as_str),
                &extra,
                false,
                cli.yes,
            )
        }
        Some(Command::Init { roots, accent }) => {
            if !roots.is_empty() {
                for r in roots {
                    if !r.is_dir() {
                        return Err(Failure::new(
                            2,
                            "usage",
                            format!("not a folder: {}", r.display()),
                        ));
                    }
                }
                let accent = accent
                    .clone()
                    .or(cfg.accent.clone())
                    .unwrap_or_else(|| "cyan".into());
                config::setting("accent", std::slice::from_ref(&accent))
                    .map_err(|e| Failure::new(2, "usage", e.to_string()))?;
                let choice = init::Choice {
                    roots: roots
                        .iter()
                        .map(|r| std::path::absolute(r).unwrap_or_else(|_| r.clone()))
                        .collect(),
                    accent,
                };
                let path = init::save(&choice).map_err(config_err)?;
                out.value("init", &serde_json::json!({ "config": path, "roots": choice.roots, "accent": choice.accent }));
                return Ok(0);
            }
            if !interactive(cli) {
                let cwd = std::env::current_dir().unwrap_or_default();
                let s = suggest::suggest(&cwd, dirs_home().as_deref());
                out.value("init-suggestions", &s);
                if !cli.json {
                    eprintln!(
                        "kiwa: no terminal; choose with: {} init --use <DIR> [--use <DIR>...]",
                        me()
                    );
                }
                return Ok(0);
            }
            Ok(if run_init(out, &cfg, false)?.is_some() {
                0
            } else {
                130
            })
        }
        Some(Command::Alias { action }) => alias(out, &cfg, action),
        Some(Command::Config { action }) => {
            match action {
                ConfigAction::Path => out.value("config-path", &config::config_path()),
                ConfigAction::Show => out.value(
                    "config",
                    &serde_json::json!({
                        "path": cfg.path,
                        "roots": cfg.roots,
                        "depth": cfg.depth,
                        "ignore": cfg.ignore,
                        "accent": cfg.accent,
                        "mouse": cfg.mouse,
                        "aliases": cfg.aliases,
                        "state": state::state_path(),
                    }),
                ),
                ConfigAction::Get { key } => {
                    let v = cfg
                        .get(key)
                        .map_err(|e| Failure::new(2, "usage", e.to_string()))?;
                    if out.json {
                        out.value("config-get", &serde_json::json!({ "key": key, "value": v }));
                    } else {
                        match &v {
                            serde_json::Value::String(s) => println!("{s}"),
                            serde_json::Value::Array(a) if a.iter().all(|x| x.is_string()) => {
                                for x in a {
                                    println!("{}", x.as_str().unwrap_or_default());
                                }
                            }
                            other => println!("{other}"),
                        }
                    }
                }
                ConfigAction::Edit => return edit_config(cli),
                ConfigAction::Set { key, values } => {
                    let v = config::setting(key, values)
                        .map_err(|e| Failure::new(2, "usage", e.to_string()))?;
                    let mut e = Editor::open_default().map_err(config_err)?;
                    e.set(key, v);
                    e.save().map_err(config_err)?;
                    out.value(
                        "config-set",
                        &serde_json::json!({ "key": key, "values": values, "path": e.path() }),
                    );
                }
            }
            Ok(0)
        }
        Some(Command::History { clear }) => {
            let mut st = State::load();
            if *clear {
                st.clear();
                st.save();
            }
            out.value("history", &st.history);
            Ok(0)
        }
        Some(Command::ShellInit { shell, name }) => {
            if let Some(why) = name
                .as_deref()
                .and_then(|n| shell::shadowing(n, &cfg.aliases, None))
            {
                return Err(Failure::new(
                    2,
                    "usage",
                    format!("{why}; choose another name for the picker"),
                ));
            }
            print!("{}", shell::init(*shell, &cfg.aliases, name.as_deref()));
            if std::io::stdout().is_terminal() {
                eprintln!(
                    "\n# add this line to your shell profile (remove it to undo):\n#   {}",
                    shell::profile_line(*shell, name.as_deref())
                );
            }
            Ok(0)
        }
        Some(Command::Completions { shell }) => {
            print!("{}", shell::completions(*shell));
            Ok(0)
        }
        Some(Command::Reset) => reset(cli, out, cli.yes),
        Some(Command::About { project }) => {
            let p = target_project(cli, &cfg, project.as_deref(), "about")?;
            let a = kiwa_core::about::about(&p);
            if out.json {
                out.value("about", &a);
            } else {
                print!("{}", render_about(&a));
            }
            Ok(0)
        }
        Some(Command::Setup { project, dry_run }) => {
            setup(cli, out, &cfg, project.as_deref(), *dry_run)
        }
        Some(Command::Doctor { .. }) => unreachable!("handled before the config is loaded"),
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("KIWA_HOME")
        .map(PathBuf::from)
        .or_else(home_dir)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Interactive setup. Returns the reloaded config on success, `None` if cancelled.
fn run_init(out: &Out, cfg: &Config, first_run: bool) -> Result<Option<Config>, Failure> {
    let cwd = std::env::current_dir().unwrap_or_default();
    if first_run {
        eprintln!("kiwa: no projects here and no configuration yet: let's set it up.");
    }
    eprint!("kiwa: looking for project folders… ");
    let _ = std::io::stderr().flush();
    let t = Instant::now();
    let s = suggest::suggest(&cwd, dirs_home().as_deref());
    eprintln!("{} found in {} ms", s.len(), t.elapsed().as_millis());
    let Some(choice) = init::wizard(&s, cfg.accent.as_deref()).map_err(internal)? else {
        eprintln!("kiwa: setup cancelled, nothing written");
        return Ok(None);
    };
    if choice.roots.is_empty() {
        eprintln!("kiwa: no folder chosen, nothing written");
        return Ok(None);
    }
    let path = init::save(&choice).map_err(config_err)?;
    if out.json {
        out.value(
            "init",
            &serde_json::json!({ "config": path, "roots": choice.roots, "accent": choice.accent }),
        );
    } else {
        eprintln!(
            "kiwa: saved {} ({} folder(s), accent {})",
            path.display(),
            choice.roots.len(),
            choice.accent
        );
    }
    config::load().map(Some).map_err(config_err)
}

fn alias(out: &Out, cfg: &Config, action: &AliasAction) -> Result<i32, Failure> {
    match action {
        AliasAction::List => out.value("aliases", &cfg.aliases),
        AliasAction::Add {
            name,
            project,
            task,
            args,
        } => {
            let valid = name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if name.is_empty() || !valid {
                return Err(Failure::new(
                    2,
                    "usage",
                    "alias names use letters, digits, - and _".into(),
                ));
            }
            if matches!(
                name.as_str(),
                "pick"
                    | "list"
                    | "ls"
                    | "info"
                    | "run"
                    | "last"
                    | "path"
                    | "init"
                    | "alias"
                    | "config"
                    | "history"
                    | "shell-init"
                    | "completions"
                    | "doctor"
                    | "setup"
                    | "about"
                    | "reset"
                    | "help"
            ) {
                return Err(Failure::new(
                    2,
                    "usage",
                    format!("`{name}` is a {} command", me()),
                ));
            }
            let mut e = Editor::open_default().map_err(config_err)?;
            e.remove("alias", Some(name));
            let mut values: Vec<kdl::KdlValue> = vec![
                name.clone().into(),
                project.clone().into(),
                task.clone().into(),
            ];
            values.extend(args.iter().cloned().map(Into::into));
            e.push("alias", values);
            e.save().map_err(config_err)?;
            out.value("alias-add", &serde_json::json!({ "name": name, "project": project, "task": task, "args": args }));
        }
        AliasAction::Remove { name } => {
            let mut e = Editor::open_default().map_err(config_err)?;
            if e.remove("alias", Some(name)) == 0 {
                return Err(
                    Failure::new(3, "not-found", format!("no alias named `{name}`"))
                        .with_suggestions(cfg.aliases.iter().map(|a| a.name.clone()).collect()),
                );
            }
            e.save().map_err(config_err)?;
            out.value("alias-remove", &serde_json::json!({ "name": name }));
        }
    }
    Ok(0)
}

/// `kw config edit`: `$VISUAL`, then `$EDITOR`, then the platform's opener. The file is
/// created (with `version 1`) if needed, and validated after the editor closes.
fn edit_config(cli: &Cli) -> Result<i32, Failure> {
    if !interactive(cli) {
        return Err(Failure::new(
            2,
            "usage",
            format!("{0} config edit needs a terminal; use {0} config set", me()),
        ));
    }
    let path = config::config_path()
        .ok_or_else(|| internal("no configuration directory on this system"))?;
    if !path.is_file() {
        let mut e = Editor::open(&path).map_err(config_err)?;
        e.save().map_err(config_err)?;
    }
    let editor = std::env::var("VISUAL")
        .ok()
        .or_else(|| std::env::var("EDITOR").ok())
        .filter(|e| !e.trim().is_empty());
    let mut cmd = match editor {
        // Editors are often configured with flags ("code --wait"): split on whitespace.
        Some(e) => {
            let mut parts = e.split_whitespace();
            let mut c = std::process::Command::new(parts.next().unwrap_or("vi"));
            c.args(parts);
            c
        }
        None if cfg!(windows) => std::process::Command::new("notepad"),
        None if cfg!(target_os = "macos") => {
            let mut c = std::process::Command::new("open");
            c.args(["-W", "-t"]);
            c
        }
        None => std::process::Command::new("vi"),
    };
    std::fs::copy(&path, config::backup_path(&path)).map_err(internal)?;
    let status = cmd
        .arg(&path)
        .status()
        .map_err(|e| Failure::new(1, "spawn", format!("cannot start the editor: {e}")))?;
    match config::load() {
        Ok(_) => eprintln!("kiwa: {} is valid", path.display()),
        Err(e) => {
            return Err(config_err(format!(
                "{e} (a copy of the previous file is in {})",
                config::backup_path(&path).display()
            )));
        }
    }
    Ok(status.code().unwrap_or(0))
}

fn reset(cli: &Cli, out: &Out, yes: bool) -> Result<i32, Failure> {
    let files: Vec<PathBuf> = [config::config_path(), state::state_path()]
        .into_iter()
        .flatten()
        .filter(|p| p.is_file())
        .collect();
    if files.is_empty() {
        out.value("reset", &serde_json::json!({ "removed": [] }));
        return Ok(0);
    }
    if !yes {
        if !interactive(cli) {
            return Err(Failure::new(
                2,
                "usage",
                format!(
                    "{} reset deletes your config and history; confirm with --yes",
                    me()
                ),
            ));
        }
        eprintln!("This deletes:");
        for f in &files {
            eprintln!("  {}", f.display());
        }
        eprint!("Continue? [y/N] ");
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).map_err(internal)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            eprintln!("kiwa: nothing deleted");
            return Ok(1);
        }
    }
    for f in &files {
        std::fs::remove_file(f).map_err(internal)?;
    }
    out.value("reset", &serde_json::json!({ "removed": files }));
    Ok(0)
}

fn run_task(
    out: &Out,
    projects: &[Project],
    project: &str,
    task: Option<&str>,
    extra: &[String],
    dry_run: bool,
    yes: bool,
) -> Result<i32, Failure> {
    let p = resolve::project(projects, project)?;
    let Some(task) = task else {
        // No task given: show what is available (agents get it as data).
        out.info(p);
        return Err(Failure::new(
            2,
            "usage",
            format!("specify a task: {} run {} <task>", me(), p.id),
        )
        .with_suggestions(p.tasks.iter().map(|t| t.name.clone()).collect()));
    };
    let t = resolve::task(p, task)?;
    if !t.params.is_empty() && extra.is_empty() && !dry_run {
        let need = t.params_usage();
        return Err(Failure::new(
            2,
            "missing-input",
            format!(
                "`{}` needs {need}: {} {} {} -- {need}",
                t.name,
                me(),
                p.id,
                t.name
            ),
        ));
    }
    if dry_run {
        out.dry_run(p, t, extra);
        return Ok(0);
    }
    if let Some(why) = exec::confirmation(t).filter(|_| !yes) {
        confirm(out, t, extra, &why)?;
    }
    out.starting(p, t, extra);
    let outcome = exec::run(p, t, extra, out.json)?;
    let mut st = State::load();
    st.record(&p.path, &t.name, Some(outcome.code));
    st.save();
    out.finished(p, t, &outcome);
    Ok(outcome.code)
}

/// The project `kiwa setup` and `kiwa about` work on: a folder, a name, or the current folder.
fn target_project(
    cli: &Cli,
    cfg: &Config,
    arg: Option<&str>,
    cmd: &str,
) -> Result<Project, Failure> {
    let folder = match arg {
        Some(a) if std::path::Path::new(a).is_dir() => Some(PathBuf::from(a)),
        Some(_) => None,
        None => Some(std::env::current_dir().map_err(internal)?),
    };
    match (folder, arg) {
        (Some(dir), _) => {
            let opts = DiscoverOptions {
                roots: vec![dir.clone()],
                max_depth: 0,
                ..Default::default()
            };
            scan_with(opts, cfg)
                .projects
                .into_iter()
                .next()
                .ok_or_else(|| {
                    Failure::new(
                        3,
                        "not-found",
                        format!(
                            "{} is not a project; name one: {} {cmd} <project>",
                            dir.display(),
                            me()
                        ),
                    )
                })
        }
        (None, Some(name)) => {
            let s = scan(cli, cfg);
            resolve::project(&s.projects, name)
                .cloned()
                .map_err(Failure::from)
        }
        (None, None) => unreachable!("no argument means the current folder"),
    }
}

/// `kiwa setup`: show the plan, confirm it once, run the steps in order, stop at a failure.
fn setup(
    cli: &Cli,
    out: &Out,
    cfg: &Config,
    arg: Option<&str>,
    dry_run: bool,
) -> Result<i32, Failure> {
    use kiwa_core::setup::Action;
    let p = target_project(cli, cfg, arg, "setup")?;
    let plan = kiwa_core::setup::plan(&p, true);
    if dry_run {
        if out.json {
            out.value("setup-plan", &plan);
        } else {
            eprint!("{}", render_plan(&plan));
        }
        return Ok(0);
    }
    if !out.json {
        eprint!("{}", render_plan(&plan));
    }
    let pending: Vec<&kiwa_core::setup::Step> = plan.pending().collect();
    if pending.is_empty() {
        if out.json {
            out.value(
                "setup",
                &serde_json::json!({ "project": p.id, "steps": [] }),
            );
        }
        return Ok(0);
    }
    if !cli.yes {
        let tty = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
        if out.json || !tty {
            return Err(Failure::new(
                2,
                "confirmation-required",
                format!(
                    "setup of `{}` has {} step(s); run it again with --yes to confirm",
                    p.id,
                    pending.len()
                ),
            ));
        }
        eprint!("Run these {} step(s)? [y/N] ", pending.len());
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).map_err(internal)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            return Err(Failure::new(1, "declined", "nothing done".into()));
        }
    }
    let mut results = Vec::new();
    for step in pending {
        let code = match &step.action {
            Action::Copy { from, to } => {
                if !to.exists() {
                    std::fs::copy(from, to).map_err(internal)?;
                }
                if !out.json {
                    eprintln!("✔ {}: {}", step.what, to.display());
                }
                0
            }
            Action::Run { task, cwd } => {
                if let Some(why) = exec::confirmation(task).filter(|_| !cli.yes) {
                    confirm(out, task, &[], &why)?;
                }
                let at = Project {
                    path: cwd.clone(),
                    ..p.clone()
                };
                out.starting(&at, task, &[]);
                let o = exec::run(&at, task, &[], out.json)?;
                out.finished(&at, task, &o);
                o.code
            }
        };
        results.push(serde_json::json!({ "what": step.what, "exit_code": code }));
        if code != 0 {
            if out.json {
                out.value(
                    "setup",
                    &serde_json::json!({ "project": p.id, "steps": results }),
                );
            }
            return Ok(code);
        }
    }
    if out.json {
        out.value(
            "setup",
            &serde_json::json!({ "project": p.id, "steps": results }),
        );
    } else {
        eprintln!("kiwa: `{}` is set up", p.id);
    }
    Ok(0)
}

/// The project card as text: identity, then how to start.
fn render_about(a: &kiwa_core::about::About) -> String {
    use std::fmt::Write as _;
    let mut s = format!("◆ {}", a.id);
    if a.name != a.id && !a.id.ends_with(&format!("/{}", a.name)) {
        let _ = write!(s, " ({})", a.name);
    }
    if let Some(d) = &a.description {
        let _ = write!(s, " · {d}");
    }
    s.push('\n');
    let kv = |s: &mut String, k: &str, v: &str| {
        let _ = writeln!(s, "  {k:<9}{v}");
    };
    if let Some(l) = &a.license {
        kv(&mut s, "licence", l);
    }
    if let Some(r) = &a.repository {
        kv(&mut s, "repo", r);
    }
    if let Some(b) = &a.branch {
        kv(&mut s, "branch", b);
    }
    kv(&mut s, "stack", &a.stack.join(", "));
    if !a.start.is_empty() || !a.setup.is_empty() {
        s.push_str("\nstart\n");
    }
    if !a.setup.is_empty() {
        kv(
            &mut s,
            "setup",
            &format!("{} setup {}   ({})", me(), a.id, a.setup.join(", ")),
        );
    }
    for r in &a.start {
        kv(&mut s, r.role, &format!("{} {} {}", me(), a.id, r.task));
    }
    if !a.requires.is_empty() {
        s.push_str("\nneeds\n");
        for r in &a.requires {
            let _ = writeln!(s, "  {:<9}{:<12} {}", r.tool, r.version, r.from);
        }
    }
    s
}

/// The plan as text: numbered steps, done ones ticked, README suggestions apart.
fn render_plan(plan: &kiwa_core::setup::Plan) -> String {
    use kiwa_core::setup::Action;
    use std::fmt::Write as _;
    let mut s = format!("kiwa setup · {}\n", plan.project);
    if plan.steps.is_empty() {
        s.push_str("  nothing to install or create here\n");
    }
    let mut n = 0;
    for step in &plan.steps {
        let what = match &step.action {
            Action::Run { task, .. } => exec::shown(task, &[]),
            Action::Copy { from, to } => format!(
                "copy {} → {}",
                from.file_name().unwrap_or_default().to_string_lossy(),
                to.file_name().unwrap_or_default().to_string_lossy()
            ),
        };
        let mark = if step.done {
            "  ✔".to_owned()
        } else {
            n += 1;
            format!("{n:>3}")
        };
        let _ = writeln!(s, "{mark} {:<22} {what:<34} {}", step.what, step.why);
    }
    if !plan.suggested.is_empty() {
        let _ = writeln!(
            s,
            "\nsuggested by the README, one at a time ({} <task>):",
            me()
        );
        for t in &plan.suggested {
            let _ = writeln!(s, "    {:<28} {}", t.name, exec::shown(t, &[]));
        }
    }
    s
}

/// Show what is about to run and why it is asked, then wait for `y`. Without a terminal (or
/// with `--json`) nothing is asked: the run is refused until `--yes` is given.
fn confirm(out: &Out, t: &kiwa_core::Task, extra: &[String], why: &str) -> Result<(), Failure> {
    let tty = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if out.json || !tty {
        return Err(Failure::new(
            2,
            "confirmation-required",
            format!("`{}` {why}; run it again with --yes to confirm", t.name),
        ));
    }
    eprintln!("kiwa: `{}` {why}:", t.name);
    eprintln!("  {}", exec::shown(t, extra));
    eprint!("Run it? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).map_err(internal)?;
    if matches!(answer.trim(), "y" | "Y" | "yes") {
        Ok(())
    } else {
        Err(Failure::new(1, "declined", format!("`{}` not run", t.name)))
    }
}

#[cfg(test)]
mod name_tests {
    #[test]
    fn the_name_follows_the_program_file() {
        assert_eq!(super::name_for(Some("kw")), "kw");
        assert_eq!(super::name_for(Some("kiwa")), "kiwa");
        assert_eq!(super::name_for(Some("kiwa-x86_64")), "kiwa");
        assert_eq!(super::name_for(None), "kiwa");
        let help = crate::cli::after_help("kiwa");
        assert!(
            help.contains("  kiwa web dev            run task"),
            "{help}"
        );
        assert!(crate::cli::after_help("kw").contains("  kw web dev              run task"));
    }

    #[test]
    fn the_picker_offers_setup_first_where_it_is_needed() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("package.json"),
            r#"{"scripts":{"setup":"x","dev":"vite"},"dependencies":{"a":"1"}}"#,
        )
        .unwrap();
        let ps = kiwa_core::discover(&kiwa_core::DiscoverOptions {
            roots: vec![d.path().to_path_buf()],
            max_depth: 0,
            ..Default::default()
        });
        let with = super::with_setup_entries(&ps);
        let first = &with[0].tasks[0];
        assert_eq!(first.name, "kiwa-setup", "the project has its own `setup`");
        assert_eq!(first.runner, "kiwa");
        assert_eq!(first.command[1], "setup");
        assert!(
            first
                .description
                .as_deref()
                .unwrap()
                .contains("install dependencies")
        );
        assert_eq!(ps[0].tasks.len() + 1, with[0].tasks.len());
        std::fs::create_dir_all(d.path().join("node_modules")).unwrap();
        let ps = kiwa_core::discover(&kiwa_core::DiscoverOptions {
            roots: vec![d.path().to_path_buf()],
            max_depth: 0,
            ..Default::default()
        });
        assert_eq!(
            super::with_setup_entries(&ps)[0].tasks.len(),
            ps[0].tasks.len(),
            "installed: no entry"
        );
    }
}
