//! Tasks people already wrote down elsewhere: `Procfile` processes, VS Code tasks, and the
//! `run:` steps of CI jobs (GitHub Actions, GitLab CI). Like script folders, these add tasks to
//! a project or a repository; they never make a project on their own. Lines that need a shell
//! become `shell` tasks, shown and confirmed before they run (ADR 0009). All static: files are
//! read with small scanners, nothing is run.

use crate::detect::{Detection, jsonc};
use crate::model::{Ecosystem, Task};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn read(dir: &Path, rel: &str) -> Option<String> {
    let s = fs::read_to_string(dir.join(rel)).ok()?;
    Some(s.trim_start_matches('\u{feff}').to_owned())
}

fn detection(ecosystem: Ecosystem, tasks: Vec<Task>, from: String) -> Option<Detection> {
    (!tasks.is_empty()).then_some(Detection {
        ecosystem,
        tasks,
        warnings: Vec::new(),
        hints: Vec::new(),
        from,
    })
}

// ---------------------------------------------------------------- Procfile

/// `web: bundle exec puma -C config/puma.rb` → task `web`.
pub fn parse_procfile(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(':'))
        .filter(|(n, c)| {
            !n.is_empty()
                && n.chars()
                    .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
                && !c.trim().is_empty()
        })
        .map(|(n, c)| (n.to_owned(), c.trim().to_owned()))
        .collect()
}

pub fn procfile(dir: &Path) -> Option<Detection> {
    let text = read(dir, "Procfile")?;
    let tasks = parse_procfile(&text)
        .into_iter()
        .map(|(name, line)| Task::from_line(name, "procfile", &line))
        .collect();
    detection(Ecosystem::Procfile, tasks, "Procfile".into())
}

// ---------------------------------------------------------------- VS Code

/// Tasks of `.vscode/tasks.json`: `process` ones as an argv, `shell` ones as a line (confirmed
/// when it needs a shell). The platform's override (`windows`, `osx`, `linux`) wins. Tasks that
/// use VS Code variables other than `${workspaceFolder}` are left out: only the editor can
/// resolve them. Other types (`npm`, `cargo`, …) are already listed by their own detector.
pub fn vscode_tasks(text: &str) -> Vec<Task> {
    let Ok(v) = serde_json::from_str::<Value>(&jsonc(text)) else {
        return Vec::new();
    };
    let platform = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };
    let mut out = Vec::new();
    for t in v
        .get("tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let over = t.get(platform);
        let pick = |k: &str| over.and_then(|o| o.get(k)).or_else(|| t.get(k));
        let Some(label) = t.get("label").and_then(Value::as_str) else {
            continue;
        };
        let kind = pick("type").and_then(Value::as_str).unwrap_or("process");
        let Some(command) = pick("command").and_then(Value::as_str) else {
            continue;
        };
        let args: Vec<String> = pick("args")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        x.as_str()
                            .map(str::to_owned)
                            .or_else(|| x.get("value")?.as_str().map(str::to_owned))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let resolve = |s: &str| {
            s.replace("${workspaceFolder}", ".")
                .replace("${workspaceRoot}", ".")
        };
        let command = resolve(command);
        let args: Vec<String> = args.iter().map(|a| resolve(a)).collect();
        if command.contains("${") || args.iter().any(|a| a.contains("${")) {
            continue;
        }
        let task = match kind {
            "process" => {
                let mut argv = vec![command];
                argv.extend(args);
                Task::from_argv(label, "vscode", argv)
            }
            "shell" => {
                let mut line = command;
                for a in &args {
                    line.push(' ');
                    if a.contains(char::is_whitespace) {
                        line.push_str(&format!("'{}'", a.replace('\'', "'\\''")));
                    } else {
                        line.push_str(a);
                    }
                }
                Task::from_line(label, "vscode", &line)
            }
            _ => continue,
        };
        let detail = t.get("detail").and_then(Value::as_str);
        out.push(match detail {
            Some(d) => task.with_description(Some(d.to_owned())),
            None => task,
        });
    }
    out
}

pub fn vscode(dir: &Path) -> Option<Detection> {
    let text = read(dir, ".vscode/tasks.json")?;
    detection(
        Ecosystem::Vscode,
        vscode_tasks(&text),
        ".vscode/tasks.json".into(),
    )
}

// ---------------------------------------------------------------- README

/// Headings whose code blocks are steps to get a project running.
const QUICK_START: &[&str] = &[
    "getting started",
    "get started",
    "quick start",
    "quickstart",
    "install",
    "installation",
    "installing",
    "setup",
    "set up",
    "usage",
    "development",
    "developing",
    "local development",
    "running",
    "run",
    "running locally",
    "build",
    "building",
    "test",
    "testing",
    "tests",
];

/// Shell commands of the README's quick-start sections, in order: fenced blocks in a POSIX shell
/// language (or none) under a heading such as Getting started, Installation or Usage. Prompts
/// (`$ `) are removed; in a block that uses them, lines without one are output and dropped.
/// Continuations are joined. `git clone` and `cd` are left out: the reader is already there.
pub fn readme_steps(text: &str) -> Vec<String> {
    /// Where the reader is: in prose, in a block whose lines are kept, or in one skipped.
    enum Fence {
        Out,
        Keep(Vec<String>),
        Skip,
    }
    let mut out = Vec::new();
    let mut in_section = false;
    let mut section_level = 0;
    let mut fence = Fence::Out;
    for line in text.lines() {
        let t = line.trim();
        let closes = t.starts_with("```") || t.starts_with("~~~");
        match &mut fence {
            Fence::Keep(lines) if closes => {
                out.extend(block_commands(lines));
                fence = Fence::Out;
                continue;
            }
            Fence::Keep(lines) => {
                lines.push(line.to_owned());
                continue;
            }
            Fence::Skip if closes => {
                fence = Fence::Out;
                continue;
            }
            Fence::Skip => continue,
            Fence::Out => {}
        }
        if t.starts_with('#') {
            let level = t.chars().take_while(|c| *c == '#').count();
            let title: String = t[level..]
                .trim()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if QUICK_START.contains(&title.as_str()) {
                in_section = true;
                section_level = level;
            } else if level <= section_level {
                in_section = false;
            }
            continue;
        }
        if let Some(info) = t.strip_prefix("```").or_else(|| t.strip_prefix("~~~")) {
            let lang = info.trim().to_lowercase();
            let posix = matches!(
                lang.as_str(),
                "" | "sh" | "bash" | "shell" | "console" | "zsh" | "shell-session" | "terminal"
            );
            fence = if in_section && posix {
                Fence::Keep(Vec::new())
            } else {
                // Another language, or outside the sections: its lines are not headings either.
                Fence::Skip
            };
        }
    }
    out
}

/// A line without its comment: from a `#` that starts a word, outside quotes, to the end.
/// READMEs annotate commands (`brew install x   # macOS`); the comment is not part of them.
fn without_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut prev_space = true;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == '#' && prev_space => return line[..i].trim_end(),
            None => {}
        }
        prev_space = c.is_whitespace();
    }
    line
}

/// The commands of one block.
fn block_commands(lines: &[String]) -> Vec<String> {
    // `$ ` is the prompt; `> ` is not (npm prints `> app@1.0.0 dev` before a script).
    let prompted = lines.iter().any(|l| l.trim_start().starts_with("$ "));
    let mut cmds: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    for raw in lines {
        let l = raw.trim();
        let l = match (prompted, l.strip_prefix("$ ")) {
            (_, Some(c)) => c.trim(),
            (true, None) if pending.is_none() => continue, // output
            _ => l,
        };
        let l = without_comment(l);
        if l.is_empty() {
            continue;
        }
        let joined = match pending.take() {
            Some(p) => format!("{p} {l}"),
            None => l.to_owned(),
        };
        match joined.strip_suffix('\\') {
            Some(head) => pending = Some(head.trim_end().to_owned()),
            None => cmds.push(joined),
        }
    }
    cmds.retain(|c| !(c.starts_with("git clone") || c == "cd" || c.starts_with("cd ")));
    cmds
}

/// A task name from a command: `readme:` and its first words (`readme:npm-install`).
fn step_name(cmd: &str, taken: &mut Vec<String>) -> String {
    let words: Vec<String> = cmd
        .split_whitespace()
        .take(3)
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '.')
                .collect::<String>()
        })
        .filter(|w| !w.is_empty() && !w.starts_with('-'))
        .collect();
    let base = format!("readme:{}", words.join("-"));
    let mut name = base.clone();
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    taken.push(name.clone());
    name
}

pub fn readme(dir: &Path) -> Option<Detection> {
    let file = ["README.md", "Readme.md", "readme.md", "README.markdown"]
        .into_iter()
        .find(|f| dir.join(f).is_file())?;
    let text = read(dir, file)?;
    let mut taken = Vec::new();
    let tasks = readme_steps(&text)
        .into_iter()
        .map(|cmd| {
            let mut t = Task::from_line(step_name(&cmd, &mut taken), "readme", &cmd);
            t.suggested = true;
            t
        })
        .collect();
    detection(Ecosystem::Readme, tasks, file.to_owned())
}

// ---------------------------------------------------------------- CI

/// A job reproduced locally: its shell lines in order, and the steps that cannot run here.
#[derive(Debug, Default, PartialEq)]
pub struct Job {
    pub name: String,
    pub lines: Vec<String>,
    pub skipped: usize,
}

impl Job {
    /// One `shell` task: `set -e` then each line, as CI runs them (stop at the first failure).
    fn task(&self, runner: &str, prefix: &str) -> Option<Task> {
        if self.lines.is_empty() {
            return None;
        }
        let script = format!("set -e\n{}", self.lines.join("\n"));
        let mut t = Task::from_line(format!("{prefix}{}", self.name), runner, &script);
        let first = self.lines[0].lines().next().unwrap_or_default();
        let more = match self.lines.len() {
            1 => String::new(),
            n => format!(" (+{} more)", n - 1),
        };
        let skipped = match self.skipped {
            0 => String::new(),
            n => format!(" · {n} step(s) not reproducible locally"),
        };
        t.description = Some(format!("{first}{more}{skipped}"));
        Some(t)
    }
}

/// Indentation of a line, in spaces.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The value of `key: value` at `lines[i]`, following a block scalar (`|`, `|-`, `>`…) onto
/// the lines indented deeper than `key_indent`. Returns the value and the next line to read.
fn scalar(lines: &[&str], i: usize, inline: &str, key_indent: usize) -> (String, usize) {
    let inline = inline.trim();
    if !inline.starts_with(['|', '>']) {
        let v = inline.trim_matches(|c| c == '"' || c == '\'');
        return (v.to_owned(), i + 1);
    }
    let folded = inline.starts_with('>');
    let mut j = i + 1;
    let mut body: Vec<&str> = Vec::new();
    let mut base: Option<usize> = None;
    while j < lines.len() {
        let l = lines[j];
        if l.trim().is_empty() {
            body.push("");
            j += 1;
            continue;
        }
        if indent(l) <= key_indent {
            break;
        }
        let b = *base.get_or_insert(indent(l));
        body.push(l.get(b..).unwrap_or(l.trim_start()));
        j += 1;
    }
    while body.last() == Some(&"") {
        body.pop();
    }
    let sep = if folded { " " } else { "\n" };
    (body.join(sep), j)
}

/// GitHub Actions: each job's `run:` steps (with `working-directory:`), one task per job.
/// `uses:` steps and steps with a non-POSIX `shell:` are counted as not reproducible.
pub fn github_jobs(text: &str) -> Vec<Job> {
    let lines: Vec<&str> = text.lines().collect();
    let mut jobs: Vec<Job> = Vec::new();
    let mut in_jobs = false;
    let mut job_level: Option<usize> = None;
    let mut step: Option<(usize, Option<String>, Option<String>, bool)> = None; // dash indent, run, dir, foreign
    let flush = |jobs: &mut Vec<Job>,
                 step: &mut Option<(usize, Option<String>, Option<String>, bool)>| {
        if let (Some((_, run, dir, foreign)), Some(job)) = (step.take(), jobs.last_mut()) {
            match run {
                Some(r) if !foreign => job.lines.push(match dir {
                    Some(d) => format!("(cd {d:?} && {r})"),
                    None => r,
                }),
                _ => job.skipped += 1,
            }
        }
    };
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            i += 1;
            continue;
        }
        let d = indent(line);
        if d == 0 {
            flush(&mut jobs, &mut step);
            in_jobs = t.starts_with("jobs:");
            job_level = None;
            i += 1;
            continue;
        }
        if !in_jobs {
            i += 1;
            continue;
        }
        let lv = *job_level.get_or_insert(d);
        if d == lv {
            flush(&mut jobs, &mut step);
            if let Some(name) = t.strip_suffix(':') {
                jobs.push(Job {
                    name: name.trim_matches(['"', '\'']).to_owned(),
                    ..Default::default()
                });
            }
            i += 1;
            continue;
        }
        // A step starts with `- ` deeper than the job; its keys follow at dash + 2.
        let (key_line, key_indent) = match t.strip_prefix("- ") {
            Some(rest) if step.as_ref().is_none_or(|s| d <= s.0) || step.is_none() => {
                flush(&mut jobs, &mut step);
                step = Some((d, None, None, false));
                (rest, d + 2)
            }
            _ => (t, d),
        };
        if let Some(s) = step.as_mut().filter(|s| key_indent > s.0)
            && let Some((key, value)) = key_line.split_once(':')
        {
            match key.trim() {
                "run" => {
                    let (v, next) = scalar(&lines, i, value, key_indent);
                    s.1 = Some(v.trim_end().to_owned());
                    i = next;
                    continue;
                }
                "working-directory" => s.2 = Some(scalar(&lines, i, value, key_indent).0),
                "uses" => s.3 = true,
                "shell" => {
                    let sh = value.trim();
                    s.3 |= !(sh.starts_with("bash") || sh.starts_with("sh"));
                }
                _ => {}
            }
        }
        i += 1;
    }
    flush(&mut jobs, &mut step);
    jobs
}

/// GitLab CI: each job's `before_script` then `script`, one task per job. Hidden jobs
/// (`.template`) and the global keywords are not jobs.
pub fn gitlab_jobs(text: &str) -> Vec<Job> {
    const KEYWORDS: &[&str] = &[
        "stages",
        "variables",
        "default",
        "include",
        "workflow",
        "image",
        "services",
        "before_script",
        "after_script",
        "cache",
    ];
    let lines: Vec<&str> = text.lines().collect();
    let mut jobs: Vec<Job> = Vec::new();
    let mut current: Option<(String, Vec<String>, Vec<String>)> = None; // name, before, script
    let mut key: Option<(&str, usize)> = None; // collecting `script`/`before_script` items
    let mut finish = |c: Option<(String, Vec<String>, Vec<String>)>| {
        if let Some((name, mut before, script)) = c
            && !script.is_empty()
        {
            before.extend(script);
            jobs.push(Job {
                name,
                lines: before,
                skipped: 0,
            });
        }
    };
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            i += 1;
            continue;
        }
        let d = indent(line);
        if d == 0 {
            finish(current.take());
            key = None;
            if let Some(name) = t.strip_suffix(':')
                && !name.starts_with('.')
                && !KEYWORDS.contains(&name)
            {
                current = Some((name.trim_matches(['"', '\'']).to_owned(), vec![], vec![]));
            }
            i += 1;
            continue;
        }
        let Some(job) = current.as_mut() else {
            i += 1;
            continue;
        };
        if let Some((k, kd)) = key {
            if d > kd || (d == kd && t.starts_with("- ")) {
                if let Some(item) = t.strip_prefix("- ") {
                    let (v, next) = scalar(&lines, i, item, d);
                    let list = if k == "script" {
                        &mut job.2
                    } else {
                        &mut job.1
                    };
                    list.push(v.trim_end().to_owned());
                    i = next;
                    continue;
                }
                i += 1;
                continue;
            }
            key = None;
        }
        if let Some((k, value)) = t.split_once(':') {
            let k = k.trim();
            if k == "script" || k == "before_script" {
                let value = value.trim();
                if value.is_empty() {
                    key = Some((if k == "script" { "script" } else { "before" }, d));
                } else {
                    let (v, next) = scalar(&lines, i, value, d);
                    let list = if k == "script" {
                        &mut job.2
                    } else {
                        &mut job.1
                    };
                    list.extend(
                        v.trim_matches(['[', ']'])
                            .split(if value.starts_with('[') { ',' } else { '\n' })
                            .map(|s| s.trim().trim_matches(['"', '\'']).to_owned())
                            .filter(|s| !s.is_empty()),
                    );
                    i = next;
                    continue;
                }
            }
        }
        i += 1;
    }
    finish(current.take());
    jobs
}

pub fn ci(dir: &Path) -> Option<Detection> {
    let mut tasks = Vec::new();
    let mut from = Vec::new();
    if let Ok(rd) = fs::read_dir(dir.join(".github/workflows")) {
        let mut files: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".yml") || n.ends_with(".yaml"))
            .collect();
        files.sort();
        for f in files {
            let Some(text) = read(dir, &format!(".github/workflows/{f}")) else {
                continue;
            };
            let stem = f.rsplit_once('.').map_or(f.as_str(), |(s, _)| s);
            let jobs = github_jobs(&text);
            if !jobs.is_empty() {
                from.push(".github/workflows/".to_owned());
            }
            tasks.extend(
                jobs.iter()
                    .filter_map(|j| j.task("github", &format!("ci:{stem}:"))),
            );
        }
    }
    if let Some(text) = read(dir, ".gitlab-ci.yml") {
        from.push(".gitlab-ci.yml".to_owned());
        tasks.extend(
            gitlab_jobs(&text)
                .iter()
                .filter_map(|j| j.task("gitlab", "ci:")),
        );
    }
    from.dedup();
    detection(Ecosystem::Ci, tasks, from.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procfile_processes() {
        let p = parse_procfile(
            "# processes\nweb: bundle exec puma -p $PORT\nworker:  sidekiq\n\nbad line\n: nothing\n",
        );
        assert_eq!(
            p,
            [
                ("web".to_owned(), "bundle exec puma -p $PORT".to_owned()),
                ("worker".into(), "sidekiq".into())
            ]
        );
    }

    #[test]
    fn vscode_process_shell_and_platform_tasks() {
        let t = vscode_tasks(
            r#"{
  // tasks
  "version": "2.0.0",
  "tasks": [
    { "label": "serve", "type": "process", "command": "python", "args": ["-m", "http.server", "8000"] },
    { "label": "check", "type": "shell", "command": "cargo clippy && cargo test", "detail": "lint then test" },
    { "label": "open", "type": "shell", "command": "code", "args": ["${workspaceFolder}/README.md"] },
    { "label": "file", "type": "shell", "command": "echo", "args": ["${file}"] },
    { "label": "npm", "type": "npm", "script": "build" },
    { "label": "os", "command": "echo default", "type": "shell",
      "windows": { "command": "echo windows" }, "linux": { "command": "echo linux" }, "osx": { "command": "echo mac" } },
  ],
}"#,
        );
        let names: Vec<_> = t.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(
            names,
            ["serve", "check", "open", "os"],
            "unresolvable and npm tasks left out"
        );
        assert_eq!(t[0].command, ["python", "-m", "http.server", "8000"]);
        assert!(t[1].shell && t[1].description.as_deref() == Some("lint then test"));
        assert_eq!(t[2].command, ["code", "./README.md"]);
        let os = t[3].command.last().unwrap();
        assert!(os != "default", "the platform override wins: {os}");
        assert!(vscode_tasks("not json").is_empty());
    }

    #[test]
    fn readme_quick_start_steps() {
        let steps = readme_steps(
            "# App\n\nIntro.\n\n```sh\nnpm run not-in-a-section\n```\n\n## Getting started\n\n```bash\ngit clone https://github.com/x/app.git\ncd app\n# install\nnpm install\ncp .env.example .env && npm run migrate\n```\n\n```python\n# not a heading\nprint(1)\n```\n\n```console\n$ npm run dev\n> app@1.0.0 dev\nready on http://localhost:3000\n$ docker compose up \\\n    -d\n```\n\n### Tests\n\n```\nnpm test\n```\n\n## License\n\n```sh\nnpm run license-check\n```\n",
        );
        assert_eq!(
            steps,
            [
                "npm install",
                "cp .env.example .env && npm run migrate",
                "npm run dev",
                "docker compose up -d",
                "npm test"
            ]
        );
        assert_eq!(
            without_comment("brew install x   # macOS"),
            "brew install x"
        );
        assert_eq!(without_comment("echo 'a # b' # c"), "echo 'a # b'");
        assert_eq!(without_comment("curl x.com/#anchor"), "curl x.com/#anchor");
        assert_eq!(without_comment("# only"), "");
        let mut taken = Vec::new();
        assert_eq!(step_name("npm run dev", &mut taken), "readme:npm-run-dev");
        assert_eq!(
            step_name("npm run dev --port 1", &mut taken),
            "readme:npm-run-dev-2"
        );
        assert_eq!(
            step_name("./gradlew build", &mut taken),
            "readme:.gradlew-build"
        );
    }

    #[test]
    fn github_jobs_from_run_steps() {
        let jobs = github_jobs(
            "name: CI\non: [push]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - name: Install\n        run: npm ci\n        working-directory: web\n      - run: |\n          npm test\n          npm run lint\n      - name: Windows only\n        shell: pwsh\n        run: Get-ChildItem\n  docs:\n    steps:\n      - run: >-\n          mkdocs build\n          --strict\n",
        );
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].name, "test");
        assert_eq!(
            jobs[0].lines,
            ["(cd \"web\" && npm ci)", "npm test\nnpm run lint"]
        );
        assert_eq!(jobs[0].skipped, 2, "checkout and the pwsh step");
        assert_eq!(jobs[1].lines, ["mkdocs build --strict"]);
        let t = jobs[0].task("github", "ci:ci:").unwrap();
        assert!(t.shell);
        assert_eq!(t.name, "ci:ci:test");
        assert!(t.line().unwrap().starts_with("set -e\n(cd"));
        assert_eq!(
            t.description.as_deref(),
            Some("(cd \"web\" && npm ci) (+1 more) · 2 step(s) not reproducible locally")
        );
    }

    #[test]
    fn gitlab_jobs_from_scripts() {
        let jobs = gitlab_jobs(
            "stages: [test]\nvariables:\n  X: 1\n.template:\n  script: echo hidden\ntest:\n  stage: test\n  before_script:\n    - npm ci\n  script:\n    - npm test\n    - |\n      npm run e2e\n      echo done\nlint:\n  script: npm run lint\nflow:\n  script: [\"a\", \"b\"]\nnoscript:\n  stage: x\n",
        );
        let names: Vec<_> = jobs.iter().map(|j| j.name.as_str()).collect();
        assert_eq!(names, ["test", "lint", "flow"]);
        assert_eq!(
            jobs[0].lines,
            ["npm ci", "npm test", "npm run e2e\necho done"]
        );
        assert_eq!(jobs[1].lines, ["npm run lint"]);
        assert_eq!(jobs[2].lines, ["a", "b"]);
    }
}
