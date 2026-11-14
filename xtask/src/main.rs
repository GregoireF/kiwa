// SPDX-License-Identifier: MIT OR Apache-2.0
//! Developer tasks: `cargo xtask <task>`. CI calls the same commands, so a green
//! `cargo xtask check` locally means a green CI (minus other operating systems).
//!
//! - `check`        fmt, clippy (-D warnings), tests, and the 90 % coverage gate when
//!   cargo-llvm-cov is installed
//! - `bench`        performance budgets on the release binary (fails when one is exceeded)
//! - `snapshot`     prints the picker rendered on examples/demo (for reviews)
//! - `sandbox`      installs this checkout into a throwaway HOME and runs kw there
//! - `demo`         renders docs/demo/demo.tape to docs/demo/demo.gif with VHS, on
//!   examples/demo, with a throwaway config and history
//! - `outdated`     pinned tools (zizmor, actionlint, dist, ship) against their latest release
//! - `dist-generate` regenerates .github/workflows/release.yml with dist, then fixes the
//!   local publish job reference (dist 0.34 writes `uses: $/...`)

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

const USAGE: &str = "usage: cargo xtask <check | bench [--markdown] | snapshot | sandbox [--reuse] [-- kw args] | demo | outdated [--markdown] | dist-generate>";

/// Performance budgets (ROADMAP 0.1 exit condition), checked on the release binary.
/// Medians of 15 runs, end to end (process start to exit), on the CI Linux runner.
const BUDGETS: &[(&str, Duration)] = &[
    ("start-up (kw --version)", Duration::from_millis(25)),
    (
        "scan examples/demo (14 projects)",
        Duration::from_millis(50),
    ),
    ("scan 200 projects at depth 2", Duration::from_millis(150)),
];

/// Tools pinned in workflows that Dependabot cannot update. Keep in sync with
/// .github/workflows (search for `# pin:`).
const PINS: &[(&str, &str, &str)] = &[
    ("zizmor", "zizmorcore/zizmor", "1.31.1"),
    ("actionlint", "rhysd/actionlint", "1.7.12"),
    ("dist", "axodotdev/cargo-dist", "0.34.0"),
    ("ship", "GregoireF/ship-it", "0.1.0"),
];
/// A release must be this old before we move to it (a hijacked release is usually
/// caught and yanked within days).
const COOLDOWN_DAYS: i64 = 7;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("check") => check(),
        Some("bench") => bench(rest),
        Some("snapshot") => snapshot(),
        Some("sandbox") => fresh_machine(rest),
        Some("demo") => demo(),
        Some("outdated") => outdated(rest),
        Some("dist-generate") => dist_generate(),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::from(2)
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn run(prog: &str, args: &[&str]) -> Result<(), String> {
    eprintln!("$ {prog} {}", args.join(" "));
    let status = Command::new(prog)
        .args(args)
        .current_dir(root())
        .status()
        .map_err(|e| format!("cannot start {prog}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{prog} {}` failed", args.join(" ")))
    }
}

fn check() -> Result<u8, String> {
    run("cargo", &["fmt", "--all", "--check"])?;
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    let has_cov = Command::new("cargo")
        .args(["llvm-cov", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if has_cov {
        run(
            "cargo",
            &[
                "llvm-cov",
                "--workspace",
                "--exclude",
                "xtask",
                "--fail-under-lines",
                "90",
            ],
        )?;
    } else {
        eprintln!("note: cargo-llvm-cov not installed, running tests without the coverage gate");
        run("cargo", &["test", "--workspace"])?;
    }
    eprintln!("check: all good");
    Ok(0)
}

fn snapshot() -> Result<u8, String> {
    run(
        "cargo",
        &[
            "test",
            "-q",
            "-p",
            "kiwa",
            "--lib",
            "--",
            "--ignored",
            "--nocapture",
            "snapshot",
        ],
    )?;
    Ok(0)
}

fn kw_release() -> Result<PathBuf, String> {
    run("cargo", &["build", "--release", "-q", "-p", "kiwa"])?;
    let exe = root()
        .join("target/release")
        .join(if cfg!(windows) { "kiwa.exe" } else { "kiwa" });
    exe.is_file()
        .then_some(exe)
        .ok_or_else(|| "release binary not found".into())
}

fn sandbox(name: &str, fresh: bool) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(name);
    if fresh && dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn median_ms(exe: &Path, args: &[&str], home: &Path, runs: usize) -> Result<Duration, String> {
    let mut times = Vec::with_capacity(runs);
    for i in 0..runs + 2 {
        let t = Instant::now();
        let out = Command::new(exe)
            .args(args)
            .env("KIWA_CONFIG", home.join("config.kdl"))
            .env("KIWA_STATE", home.join("state.json"))
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        let d = t.elapsed();
        if !out.status.success() {
            return Err(format!(
                "kw {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        if i >= 2 {
            times.push(d); // first two runs warm the file cache
        }
    }
    times.sort();
    Ok(times[times.len() / 2])
}

fn bench(args: &[String]) -> Result<u8, String> {
    let markdown = args.iter().any(|a| a == "--markdown");
    let exe = kw_release()?;
    let dir = sandbox("kiwa-bench", true)?;
    // 200 projects at depth 2, mixed ecosystems, in 20 groups.
    let big = dir.join("big");
    for i in 0..200 {
        let p = big
            .join(format!("group{}", i / 10))
            .join(format!("project{i}"));
        match i % 4 {
            0 => write(
                &p.join("package.json"),
                r#"{"scripts":{"dev":"x","build":"y","test":"z"}}"#,
            )?,
            1 => write(&p.join("Cargo.toml"), "[package]\nname = \"p\"\n")?,
            2 => write(&p.join("go.mod"), "module p\n")?,
            _ => write(&p.join("Makefile"), "build:\n\techo\ntest:\n\techo\n")?,
        }
    }
    let demo = root().join("examples/demo");
    let demo_s = demo.to_string_lossy().into_owned();
    let big_s = big.to_string_lossy().into_owned();
    let cases: [(&str, Vec<&str>); 3] = [
        (BUDGETS[0].0, vec!["--version"]),
        (BUDGETS[1].0, vec!["-w", &demo_s, "list", "--json"]),
        (
            BUDGETS[2].0,
            vec!["-w", &big_s, "--depth", "2", "list", "--json"],
        ),
    ];
    let mut failed = false;
    let mut rows = Vec::new();
    for ((label, argv), (_, budget)) in cases.iter().zip(BUDGETS) {
        let m = median_ms(&exe, argv, &dir, 15)?;
        let ok = m <= *budget;
        failed |= !ok;
        rows.push((label, m, budget, ok));
    }
    if markdown {
        println!("| Measure | Median | Budget | |\n|---|---|---|---|");
    }
    for (label, m, budget, ok) in rows {
        let (m, b) = (m.as_secs_f64() * 1e3, budget.as_secs_f64() * 1e3);
        let mark = if ok { "✔" } else { "✘ over budget" };
        if markdown {
            println!("| {label} | {m:.1} ms | {b:.0} ms | {mark} |");
        } else {
            println!("{label:<36} {m:>7.1} ms  (budget {b:.0} ms)  {mark}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(u8::from(failed))
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Fresh-machine rehearsal: installs this checkout into a throwaway HOME seeded with sample
/// projects under `dev/` and `projects/`, then launches it. The real config is never read.
fn fresh_machine(args: &[String]) -> Result<u8, String> {
    let reuse = args.iter().any(|a| a == "--reuse");
    let kw_args: Vec<&str> = args
        .iter()
        .skip_while(|a| *a != "--")
        .skip(1)
        .map(String::as_str)
        .collect();
    let dir = sandbox("kiwa-sandbox", !reuse)?;
    let home = dir.join("home");
    let bin_root = dir.join("root");
    if !reuse {
        let crate_dir = root().join("crates/kiwa");
        run(
            "cargo",
            &[
                "install",
                "--quiet",
                "--locked",
                "--path",
                &crate_dir.to_string_lossy(),
                "--root",
                &bin_root.to_string_lossy(),
            ],
        )?;
        std::fs::create_dir_all(home.join("dev")).map_err(|e| e.to_string())?;
        copy_dir(&root().join("examples/demo"), &home.join("dev"))?;
        write(
            &home.join("projects/notes/.git/HEAD"),
            "ref: refs/heads/main\n",
        )?;
    }
    let exe = bin_root
        .join("bin")
        .join(if cfg!(windows) { "kiwa.exe" } else { "kiwa" });
    eprintln!(
        "sandbox: {} (keep it for the next run with --reuse)",
        dir.display()
    );
    let status = Command::new(exe)
        .args(&kw_args)
        .current_dir(&home)
        .env("KIWA_CONFIG", dir.join("config.kdl"))
        .env("KIWA_STATE", dir.join("state.json"))
        .env("KIWA_HOME", &home)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(u8::try_from(status.code().unwrap_or(1)).unwrap_or(1))
}

/// The image `cargo xtask demo` builds from docs/demo/Dockerfile.
const DEMO_IMAGE: &str = "kiwa-demo";

/// Record docs/demo/demo.gif in Docker, like ship-it: kiwa built from this checkout on top of
/// the VHS image (ttyd and ffmpeg included), the repository mounted at /vhs. Nothing to install
/// but Docker; the config and history the demo uses stay in the container.
fn demo() -> Result<u8, String> {
    let has_docker = Command::new("docker")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !has_docker {
        return Err("Docker is not running: https://docs.docker.com/get-docker".into());
    }
    run(
        "docker",
        &["build", "-f", "docs/demo/Dockerfile", "-t", DEMO_IMAGE, "."],
    )?;
    let mount = format!("{}:/vhs", root().display());
    run(
        "docker",
        &[
            "run",
            "--rm",
            "-v",
            &mount,
            DEMO_IMAGE,
            "docs/demo/demo.tape",
        ],
    )?;
    eprintln!("wrote docs/demo/demo.gif");
    Ok(0)
}

fn dist_generate() -> Result<u8, String> {
    run("dist", &["generate"])?;
    let path = root().join(".github/workflows/release.yml");
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let fixed = text.replace("uses: $/.github/workflows/", "uses: ./.github/workflows/");
    if fixed != text {
        std::fs::write(&path, fixed).map_err(|e| e.to_string())?;
        eprintln!(
            "fixed the local publish job reference in {}",
            path.display()
        );
    }
    Ok(0)
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` prefix (Howard Hinnant's algorithm).
fn days(date: &str) -> Option<i64> {
    let mut it = date.get(..10)?.split('-').map(|x| x.parse::<i64>());
    let (y, m, d) = (it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn version_tuple(v: &str) -> Vec<u64> {
    v.trim_start_matches('v')
        .split(['.', '-'])
        .map_while(|x| x.parse().ok())
        .collect()
}

/// Exit 0 when every pin is current, 1 when one is behind (and out of its cooldown).
/// Uses the GitHub CLI (`gh api`), so it works with the CI token's rate limit.
fn outdated(args: &[String]) -> Result<u8, String> {
    let markdown = args.iter().any(|a| a == "--markdown");
    let today = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    let mut behind = false;
    if markdown {
        println!("| Tool | Pinned | Latest | |\n|---|---|---|---|");
    }
    for (tool, repo, pinned) in PINS {
        let out = Command::new("gh")
            .args([
                "api",
                &format!("repos/{repo}/releases/latest"),
                "--jq",
                ".tag_name + \" \" + .published_at",
            ])
            .output()
            .map_err(|e| format!("cannot run gh: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "gh api failed for {repo}: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut parts = text.split_whitespace();
        let (tag, published) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        let age = days(published).map_or(i64::MAX, |d| today - d);
        let newer = version_tuple(tag) > version_tuple(pinned);
        let state = if !newer {
            "up to date".to_owned()
        } else if age < COOLDOWN_DAYS {
            format!("newer, in cooldown ({age} d)")
        } else {
            behind = true;
            "**behind**".to_owned()
        };
        if markdown {
            println!("| {tool} | {pinned} | {tag} | {state} |");
        } else {
            println!("{tool:<12} pinned {pinned:<10} latest {tag:<12} {state}");
        }
    }
    Ok(u8::from(behind))
}

#[cfg(test)]
mod tests {
    /// The demo writes its own config in the container: the `shop` workspace the switcher shows.
    #[test]
    fn the_demo_sets_up_its_own_config() {
        let tape = std::fs::read_to_string(super::root().join("docs/demo/demo.tape")).unwrap();
        assert!(tape.contains("KIWA_CONFIG=/tmp/kiwa.kdl"));
        assert!(tape.contains(r#"workspace "shop""#));
        let docker = std::fs::read_to_string(super::root().join("docs/demo/Dockerfile")).unwrap();
        assert!(docker.contains("ghcr.io/charmbracelet/vhs:"));
    }

    /// The tape only uses commands VHS knows, and writes where the README expects the GIF.
    #[test]
    fn the_demo_tape_is_well_formed() {
        let tape = std::fs::read_to_string(super::root().join("docs/demo/demo.tape")).unwrap();
        const COMMANDS: &[&str] = &[
            "Output", "Set", "Type", "Sleep", "Enter", "Hide", "Show", "Down", "Up", "Left",
            "Right", "Tab", "Escape", "Ctrl+U",
        ];
        for line in tape.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let cmd = line.split_whitespace().next().unwrap();
            assert!(COMMANDS.contains(&cmd), "unknown VHS command: {line}");
        }
        assert!(tape.contains("Output docs/demo/demo.gif"));
    }

    #[test]
    fn dates_and_versions() {
        assert_eq!(super::days("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(super::days("2026-11-08"), Some(20_765));
        assert!(super::version_tuple("v1.10.0") > super::version_tuple("1.9.3"));
        assert_eq!(super::version_tuple("0.1.0-rc.1"), [0, 1, 0]);
    }
}
