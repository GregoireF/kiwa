//! Process execution. Argv is passed as-is. A `shell` task's argv is `sh -c <line> sh`: the
//! shell is named in the task, shown, and confirmed before it runs (ADR 0009).

use crate::output::Failure;
use kiwa_core::model::Separator;
use kiwa_core::{Project, Task};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

pub struct Outcome {
    pub code: i32,
    pub duration: Duration,
}

/// Full argv including extra args. Runners that need `--` before forwarded args get it.
pub fn argv(t: &Task, extra: &[String]) -> Vec<String> {
    let mut v = t.command.clone();
    if !extra.is_empty() {
        if t.separator == Separator::Always {
            v.push("--".into());
        }
        v.extend(extra.iter().cloned());
    }
    v
}

/// Why a task is confirmed before it runs, or `None`: a shell line, a risky name, or both.
pub fn confirmation(t: &Task) -> Option<String> {
    let mut why: Vec<&str> = Vec::new();
    if t.risky {
        why.push("looks risky");
    }
    if t.suggested {
        why.push("is a step from the README");
    }
    if t.shell {
        why.push("runs a shell line");
    }
    match why.as_slice() {
        [] => None,
        [one] => Some((*one).to_owned()),
        [init @ .., last] => Some(format!("{} and {last}", init.join(", "))),
    }
}

/// What is shown before a run: the shell line as written for a `shell` task, the argv otherwise.
pub fn shown(t: &Task, extra: &[String]) -> String {
    let (Some(at), Some(line)) = (t.shell_at(), t.line()) else {
        return display_argv(&argv(t, extra));
    };
    let mut s = display_argv(&t.command[..at]);
    if !s.is_empty() {
        s.push(' ');
    }
    s.push_str(&format!("sh -c {line:?}"));
    if !extra.is_empty() {
        s.push_str(&format!(" sh {}", display_argv(extra)));
    }
    s
}

/// The port of a `http://localhost:<port>` URL.
pub fn url_port(url: &str) -> Option<u16> {
    url.rsplit(':').next()?.trim_end_matches('/').parse().ok()
}

/// Whether something already listens on `port` on this machine (a local connection attempt;
/// nothing leaves the machine).
pub fn port_busy(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok()
}

/// Lines printed before a run of a task that serves on a URL: the URL (terminals make it
/// clickable), and a warning when its port is already taken.
/// `last` gives when a task last ran (from the history), to name who probably holds the port.
pub fn url_notes(
    t: &Task,
    projects: &[Project],
    last: impl Fn(&std::path::Path, &str) -> Option<u64>,
) -> Vec<String> {
    let Some(url) = &t.url else {
        return Vec::new();
    };
    let mut v = vec![format!("→ {url}")];
    let Some(port) = url_port(url).filter(|p| port_busy(*p)) else {
        return v;
    };
    let holder = projects
        .iter()
        .flat_map(|p| p.tasks.iter().map(move |t| (p, t)))
        .filter(|(_, x)| x.url.as_deref().and_then(url_port) == Some(port))
        .filter_map(|(p, x)| {
            last(&p.path, &x.name).map(|at| (at, format!("{} › {}", p.id, x.name)))
        })
        .max()
        .map(|(_, who)| format!(", probably by {who} (the last task run on it)"))
        .unwrap_or_default();
    v.push(format!(
        "⚠ port {port} is already in use{holder}: the server may fail to start or pick another one"
    ));
    v
}

/// Open a URL in the default browser, without waiting for it.
pub fn open_url(url: &str) -> std::io::Result<()> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("explorer");
        c.arg(url);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// The program to start: `prog` from `PATH`, and for a shell line on Windows the `sh.exe` of
/// Git for Windows, which its installer does not always put on `PATH`.
fn resolve(prog: &str, shell: bool) -> Option<std::path::PathBuf> {
    which::which(prog).ok().or_else(|| {
        // A `.ps1` script asks for PowerShell 7; Windows always has Windows PowerShell.
        if cfg!(windows) && prog == "pwsh" {
            return which::which("powershell").ok();
        }
        if !(shell && cfg!(windows) && prog == "sh") {
            return None;
        }
        ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"]
            .iter()
            .filter_map(std::env::var_os)
            .flat_map(|base| {
                let base = std::path::PathBuf::from(base);
                [
                    base.join("Git/bin/sh.exe"),
                    base.join("Git/usr/bin/sh.exe"),
                    base.join("Programs/Git/bin/sh.exe"),
                ]
            })
            .find(|p| p.is_file())
    })
}

pub fn display_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.contains([' ', '"']) || a.is_empty() {
                format!("{a:?}")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn run(p: &Project, t: &Task, extra: &[String], json: bool) -> Result<Outcome, Failure> {
    let argv = argv(t, extra);
    let (prog, args) = argv
        .split_first()
        .ok_or_else(|| Failure::new(1, "internal", "empty command".into()))?;
    // `which` honours PATHEXT on Windows (npm.cmd, gradlew.bat, ...).
    let resolved = resolve(prog, t.shell).ok_or_else(|| {
        Failure::new(
            127,
            "runner-missing",
            format!("`{prog}` is not installed or not on PATH"),
        )
        .with_suggestions(vec![install_hint(prog)])
    })?;
    let mut cmd = Command::new(resolved);
    cmd.args(args).current_dir(&p.path);
    if json {
        // Keep stdout reserved for the JSON envelope; task output goes to stderr.
        cmd.stdout(Stdio::from(std::io::stderr()));
    }
    let start = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| Failure::new(1, "spawn", format!("cannot start `{prog}`: {e}")))?;
    let guard = guard::attach(&child);
    let status = child
        .wait()
        .map_err(|e| Failure::new(1, "spawn", format!("`{prog}` could not be waited for: {e}")))?;
    drop(guard);
    Ok(Outcome {
        code: exit_code(status),
        duration: start.elapsed(),
    })
}

/// No child left behind (Ctrl+C is not handled here: the task receives it directly from the
/// terminal, and kw ignores it while waiting).
///
/// - Unix: SIGTERM, SIGHUP and SIGQUIT sent to kw are forwarded to the task, then kw waits
///   for it and exits with its status, as a shell does for its foreground job.
/// - Windows: the task is put in a job object that kills the whole process tree when the
///   job's last handle closes, i.e. when kw exits or is killed (Task Manager, `taskkill /f`).
mod guard {
    use std::process::Child;

    #[cfg(unix)]
    pub struct Guard(signal_hook::iterator::Handle);

    #[cfg(unix)]
    pub fn attach(child: &Child) -> Option<Guard> {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        use signal_hook::consts::{SIGHUP, SIGQUIT, SIGTERM};
        let pid = Pid::from_raw(i32::try_from(child.id()).ok()?);
        let mut signals = signal_hook::iterator::Signals::new([SIGTERM, SIGHUP, SIGQUIT]).ok()?;
        let handle = signals.handle();
        std::thread::spawn(move || {
            for sig in signals.forever() {
                if let Ok(s) = Signal::try_from(sig) {
                    let _ = kill(pid, s);
                }
            }
        });
        Some(Guard(handle))
    }

    #[cfg(unix)]
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.close();
        }
    }

    #[cfg(windows)]
    pub struct Guard(#[allow(dead_code)] win32job::Job);

    #[cfg(windows)]
    pub fn attach(child: &Child) -> Option<Guard> {
        use std::os::windows::io::AsRawHandle;
        let job = win32job::Job::create().ok()?;
        let mut info = job.query_extended_limit_info().ok()?;
        info.limit_kill_on_job_close();
        job.set_extended_limit_info(&info).ok()?;
        job.assign_process(child.as_raw_handle() as isize).ok()?;
        Some(Guard(job))
    }

    #[cfg(not(any(unix, windows)))]
    pub struct Guard;

    #[cfg(not(any(unix, windows)))]
    pub fn attach(_: &Child) -> Option<Guard> {
        None
    }
}

/// One actionable line per runner, so a missing tool is a 10-second fix, not a search.
pub fn install_hint(prog: &str) -> String {
    let name = std::path::Path::new(prog)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        "npm" | "npx" | "node" => "install Node.js: https://nodejs.org".into(),
                // Corepack ships with Node.js up to 24 only (nodejs.org/api/corepack.html).
        "pnpm" | "yarn" if which::which("corepack").is_ok() => {
            format!("corepack enable {name}   (or: npm install --global {name})")
        }
        "pnpm" | "yarn" => format!(
            "npm install --global {name}   (Corepack is not bundled with Node.js 25+: npm install --global corepack)"
        ),
        "bun" => "install Bun: https://bun.sh".into(),
        "deno" => "install Deno: https://deno.com".into(),
        "cargo" => "install Rust: https://rustup.rs".into(),
        "go" => "install Go: https://go.dev/dl".into(),
        "uv" => "install uv: https://docs.astral.sh/uv".into(),
        "poetry" | "pdm" | "hatch" | "poe" => format!("uv tool install {name}"),
        "python" | "python3" | "pytest" => {
            "install Python, or use uv: https://docs.astral.sh/uv".into()
        }
        "pwsh" => "install PowerShell: https://aka.ms/powershell".into(),
        "bash" | "zsh" => format!("install {name} (on Windows it comes with Git for Windows: https://git-scm.com)"),
        "just" => "cargo install just   (or: winget install Casey.Just / brew install just)".into(),
        "task" => "install Task: https://taskfile.dev/installation".into(),
        "mise" => "install mise: https://mise.jdx.dev".into(),
        "make" => "install make (winget install ezwinports.make / xcode-select --install / apt install make)".into(),
        "gradle" => "add the Gradle wrapper (gradlew) or install Gradle: https://gradle.org/install".into(),
        "mvn" => "add the Maven wrapper (mvnw) or install Maven: https://maven.apache.org".into(),
        "dotnet" => "install the .NET SDK: https://dot.net".into(),
        "composer" => "install Composer: https://getcomposer.org".into(),
        "bundle" | "rake" => "gem install bundler rake".into(),
        "docker" => "install Docker: https://docs.docker.com/get-docker".into(),
        "sh" => "this task is a shell line: install a POSIX shell (on Windows, Git for Windows: https://git-scm.com)".into(),
        "devcontainer" => "npm install --global @devcontainers/cli".into(),
        _ => format!("install `{prog}` and make sure it is on PATH"),
    }
}

/// Human description of an exit code, naming interruptions instead of showing raw numbers.
/// Unix: 128 + N means "killed by signal N" (POSIX shell convention). Windows: 0xC000013A is
/// STATUS_CONTROL_C_EXIT, what a console process returns when stopped with Ctrl+C.
pub fn describe_exit(code: i32) -> String {
    const SIGNALS: &[(i32, &str)] = &[
        (1, "SIGHUP"),
        (2, "SIGINT"),
        (3, "SIGQUIT"),
        (6, "SIGABRT"),
        (9, "SIGKILL"),
        (13, "SIGPIPE"),
        (15, "SIGTERM"),
    ];
    if cfg!(windows) && code as u32 == 0xC000_013A {
        return "interrupted (Ctrl+C)".into();
    }
    if cfg!(unix) {
        if let Some((_, name)) = SIGNALS.iter().find(|(n, _)| code == 128 + n) {
            return format!("exit {code} ({name})");
        }
    }
    format!("exit {code}")
}

fn exit_code(s: ExitStatus) -> i32 {
    if let Some(c) = s.code() {
        return c;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = s.signal() {
            return 128 + sig;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_descriptions() {
        assert_eq!(describe_exit(0), "exit 0");
        assert_eq!(describe_exit(3), "exit 3");
        #[cfg(windows)]
        assert_eq!(describe_exit(0xC000_013Au32 as i32), "interrupted (Ctrl+C)");
        #[cfg(unix)]
        assert_eq!(describe_exit(130), "exit 130 (SIGINT)");
    }

    #[test]
    fn every_known_runner_has_an_actionable_hint() {
        for r in [
            "npm", "node", "pnpm", "yarn", "bun", "deno", "cargo", "go", "uv", "poetry", "pdm",
            "hatch", "poe", "python", "pytest", "just", "task", "mise", "make", "gradle", "mvn",
            "dotnet", "composer", "bundle", "rake", "docker",
        ] {
            let h = install_hint(r);
            assert!(
                !h.contains("make sure it is on PATH"),
                "{r} has no specific hint"
            );
        }
        assert!(
            install_hint("C:/x/gradlew.bat").contains("Gradle")
                || install_hint("C:/x/gradlew.bat").contains("PATH")
        );
        assert!(install_hint("frobnicate").contains("frobnicate"));
        assert!(
            install_hint("/usr/bin/pnpm").contains("install --global pnpm"),
            "path stem is used"
        );
    }

    #[test]
    fn argv_and_display() {
        let mut t = Task::new("t", "npm", &["npm", "run", "t"]);
        assert_eq!(argv(&t, &[]), ["npm", "run", "t"]);
        t.separator = Separator::Always;
        assert_eq!(argv(&t, &["-x".into()]), ["npm", "run", "t", "--", "-x"]);
        assert_eq!(argv(&t, &[]), ["npm", "run", "t"], "no dangling --");
        assert_eq!(
            display_argv(&["a".into(), "b c".into(), "".into()]),
            r#"a "b c" """#
        );
    }

    #[test]
    fn busy_ports_are_reported_with_their_last_user() {
        assert_eq!(url_port("http://localhost:5173"), Some(5173));
        assert_eq!(url_port("http://localhost:8080/"), Some(8080));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_busy(port));
        let mut t = Task::new("dev", "npm", &["npm", "run", "dev"]);
        t.url = Some(format!("http://localhost:{port}"));
        let mut other = t.clone();
        other.name = "serve".into();
        let p = Project {
            id: "web".into(),
            name: "web".into(),
            path: "/ws/web".into(),
            ecosystems: vec![],
            branch: None,
            default_branch: None,
            tasks: vec![t.clone(), other],
            warnings: vec![],
            hints: vec![],
            detected_from: vec![],
            member_of: None,
        };
        let last = |_: &std::path::Path, name: &str| (name == "serve").then_some(10);
        let notes = url_notes(&t, std::slice::from_ref(&p), last);
        assert_eq!(notes[0], format!("→ http://localhost:{port}"));
        assert!(
            notes[1].contains("already in use, probably by web › serve"),
            "{notes:?}"
        );
        drop(listener);
        let free = url_notes(&t, &[], |_, _| None);
        assert_eq!(free.len(), 1, "a free port: just the URL");
        assert!(url_notes(&Task::new("x", "y", &["y"]), &[], |_, _| None).is_empty());
    }

    #[test]
    fn confirmations_and_what_is_shown() {
        let plain = Task::from_line("t", "x", "cargo test");
        assert_eq!(confirmation(&plain), None);
        assert_eq!(shown(&plain, &["-q".into()]), "cargo test -q");
        let mut sh = Task::from_line("t", "x", "a && b");
        assert_eq!(confirmation(&sh).as_deref(), Some("runs a shell line"));
        assert_eq!(shown(&sh, &[]), r#"sh -c "a && b""#);
        assert_eq!(shown(&sh, &["1".into()]), r#"sh -c "a && b" sh 1"#);
        let inside = Task::from_line_in("t", "x", &["devcontainer", "exec"], "a | b");
        assert_eq!(shown(&inside, &[]), r#"devcontainer exec sh -c "a | b""#);
        sh.risky = true;
        assert!(
            confirmation(&sh)
                .unwrap()
                .contains("risky and runs a shell line")
        );
        let mut r = Task::new("deploy", "npm", &["npm", "run", "deploy"]);
        r.risky = true;
        assert_eq!(confirmation(&r).as_deref(), Some("looks risky"));
        let mut s = Task::from_line("readme:npm-ci", "readme", "npm ci");
        s.suggested = true;
        assert_eq!(
            confirmation(&s).as_deref(),
            Some("is a step from the README")
        );
        s.risky = true;
        s.shell = true;
        assert_eq!(
            confirmation(&s).as_deref(),
            Some("looks risky, is a step from the README and runs a shell line")
        );
        assert!(install_hint("sh").contains("POSIX shell"));
    }

    /// Measured on the real shell, like the runners: a line with operators and `$@` runs whole
    /// through `sh -c`, and extra arguments arrive as positional parameters. Skipped where no
    /// `sh` can be found (CI runs it on Linux, macOS and Windows with Git for Windows).
    #[test]
    fn a_shell_line_runs_in_sh_with_arguments_as_positional_parameters() {
        if resolve("sh", true).is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.txt");
        let line = format!(
            "echo first > '{0}' && echo \"args:$#:$1\" >> '{0}'",
            out.display().to_string().replace('\\', "/")
        );
        let t = Task::from_line("t", "sh", &line);
        assert!(t.shell);
        let p = Project {
            id: "p".into(),
            name: "p".into(),
            path: dir.path().to_path_buf(),
            ecosystems: vec![],
            branch: None,
            default_branch: None,
            tasks: vec![],
            warnings: vec![],
            hints: vec![],
            detected_from: vec![],
            member_of: None,
        };
        let o = run(&p, &t, &["one".into()], true).unwrap_or_else(|f| panic!("{}", f.message));
        assert_eq!(o.code, 0);
        let text = std::fs::read_to_string(out).unwrap();
        assert_eq!(text.lines().collect::<Vec<_>>(), ["first", "args:1:one"]);
    }
}
