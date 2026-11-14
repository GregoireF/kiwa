//! End-to-end tests of the `kw` binary. Every test runs in its own temp folder with its own
//! config, state and home directory, so nothing touches the developer's machine.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Env {
    dir: tempfile::TempDir,
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Out {
    fn json(&self) -> Value {
        serde_json::from_str(self.stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "stdout is not one JSON document ({e}):\n{}\nstderr:\n{}",
                self.stdout, self.stderr
            )
        })
    }
    fn all(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("ws")).unwrap();
        std::fs::create_dir_all(dir.path().join("home")).unwrap();
        Self { dir }
    }
    fn ws(&self) -> PathBuf {
        self.dir.path().join("ws")
    }
    fn config(&self) -> PathBuf {
        self.dir.path().join("config.kdl")
    }
    fn state(&self) -> PathBuf {
        self.dir.path().join("state.json")
    }
    fn file(&self, rel: &str, content: &str) -> &Self {
        let p = self.ws().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
        self
    }
    fn dir(&self, rel: &str) -> &Self {
        std::fs::create_dir_all(self.ws().join(rel)).unwrap();
        self
    }
    fn cmd(&self) -> Command {
        self.cmd_for(env!("CARGO_BIN_EXE_kw"))
    }
    fn cmd_for(&self, bin: &str) -> Command {
        let mut c = Command::new(bin);
        c.env("KIWA_CONFIG", self.config())
            .env("KIWA_STATE", self.state())
            .env("KIWA_HOME", self.dir.path().join("home"))
            .env("NO_COLOR", "1")
            .env_remove("KIWA_JSON")
            .current_dir(self.ws())
            .stdin(Stdio::null());
        c
    }
    fn kw(&self, args: &[&str]) -> Out {
        let o = self.cmd().args(args).output().expect("run kw");
        Out {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }
    fn ids(&self, extra: &[&str]) -> Vec<String> {
        let mut args = vec!["list", "--json"];
        args.extend_from_slice(extra);
        self.kw(&args).json()["data"]["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_owned())
            .collect()
    }
    fn project(&self, id: &str) -> Value {
        self.kw(&["info", id, "--json"]).json()["data"].clone()
    }
}

fn has_npm() -> bool {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    Command::new(npm)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo")
}

fn task_names(p: &Value) -> Vec<String> {
    p["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect()
}

const NODE_PKG: &str = r#"{ "scripts": {
  "ok": "node -e \"console.log('RAN-OK')\"",
  "fail": "node -e \"process.exit(3)\"",
  "show": "node show.js"
} }"#;
const SHOW_JS: &str = r#"console.log("ARGS=" + JSON.stringify(process.argv.slice(2)))"#;

fn node_project(env: &Env, name: &str) {
    env.file(&format!("{name}/package.json"), NODE_PKG)
        .file(&format!("{name}/show.js"), SHOW_JS)
        .file(&format!("{name}/package-lock.json"), "{}");
}

// ======================================================================== basics

#[test]
fn about_says_what_a_project_is_and_how_to_start() {
    let env = Env::new();
    env.file(
        "shop/package.json",
        r#"{"description":"The shop.","license":"MIT","scripts":{"dev":"vite","test":"vitest"},"dependencies":{"a":"1"}}"#,
    )
    .file("shop/package-lock.json", "{}")
    .file("shop/.nvmrc", "22\n");
    let j = env.kw(&["about", "shop", "--json"]).json();
    assert_eq!(j["kind"], "about");
    let d = &j["data"];
    assert_eq!(d["description"], "The shop.");
    assert_eq!(d["license"], "MIT");
    assert_eq!(d["start"][0]["role"], "dev");
    assert_eq!(d["setup"][0], "install dependencies");
    assert_eq!(d["requires"][0]["tool"], "node");
    let o = env.kw(&["about", "shop"]);
    assert_eq!(o.code, 0);
    for part in [
        "◆ shop · The shop.",
        "licence  MIT",
        "kw shop dev",
        "kw setup shop",
        "needs",
        "node     22",
    ] {
        assert!(o.stdout.contains(part), "{part}: {}", o.stdout);
    }
    assert_eq!(env.kw(&["about", "nope"]).code, 3);
}

#[test]
fn setup_shows_the_plan_confirms_it_and_runs_it() {
    let env = Env::new();
    env.file("app/.env.example", "PORT=3000\n")
        .file("app/.git/HEAD", "ref: refs/heads/main\n")
        .file("app/README.md", "## Usage\n```sh\nnpm run dev\n```\n");
    let plan = env.kw(&["setup", "app", "-n", "--json"]).json();
    assert_eq!(plan["kind"], "setup-plan");
    assert_eq!(plan["data"]["steps"][0]["what"], "create .env");
    assert_eq!(plan["data"]["steps"][0]["kind"], "copy");
    assert_eq!(plan["data"]["suggested"][0]["name"], "readme:npm-run-dev");
    let o = env.kw(&["setup", "app"]);
    assert_eq!(o.code, 2, "no terminal, no --yes: {}", o.all());
    assert!(
        o.stderr.contains("create .env") && o.stderr.contains("--yes"),
        "{}",
        o.stderr
    );
    assert!(!env.ws().join("app/.env").exists());
    let o = env.kw(&["setup", "app", "--yes"]);
    assert_eq!(o.code, 0, "{}", o.all());
    assert_eq!(
        std::fs::read_to_string(env.ws().join("app/.env")).unwrap(),
        "PORT=3000\n"
    );
    std::fs::write(env.ws().join("app/.env"), "PORT=1\n").unwrap();
    let o = env.kw(&["setup", "app", "--yes"]);
    assert_eq!(o.code, 0);
    assert!(o.stderr.contains("✔ create .env"), "{}", o.stderr);
    assert_eq!(
        std::fs::read_to_string(env.ws().join("app/.env")).unwrap(),
        "PORT=1\n",
        "never overwritten"
    );
    // A folder works as well as a name; outside any project, the error says what to do.
    assert_eq!(
        env.kw(&["setup", &env.ws().join("app").display().to_string(), "-n"])
            .code,
        0
    );
    let o = env.kw(&["setup"]);
    assert_eq!(o.code, 3);
    assert!(o.stderr.contains("setup <project>"), "{}", o.stderr);
}

#[test]
fn risky_tasks_need_yes_without_a_terminal() {
    if !has_npm() {
        return;
    }
    let env = Env::new();
    env.file(
        "app/package.json",
        r#"{"scripts":{"deploy":"node -e \"console.log('DEPLOYED')\""}}"#,
    )
    .file("app/package-lock.json", "{}");
    assert_eq!(env.project("app")["tasks"][0]["risky"], true);
    let o = env.kw(&["app", "deploy"]);
    assert_eq!(o.code, 2, "{}", o.all());
    assert!(
        o.stderr
            .contains("`deploy` looks risky; run it again with --yes"),
        "{}",
        o.stderr
    );
    assert!(!o.all().contains("DEPLOYED"));
    let j = env.kw(&["run", "app", "deploy", "--json"]).json();
    assert_eq!(j["error"]["kind"], "confirmation-required");
    assert_eq!(
        env.kw(&["run", "app", "deploy", "-n"]).code,
        0,
        "a dry run needs no --yes"
    );
    let o = env.kw(&["app", "deploy", "--yes"]);
    assert_eq!(o.code, 0, "{}", o.all());
    assert!(o.all().contains("DEPLOYED"));
    // The list is a config key.
    std::fs::write(env.config(), "risky \"ship\"\n").unwrap();
    assert!(env.project("app")["tasks"][0].get("risky").is_none());
}

/// `kiwa` and `kw` are the same program; help and errors use the name that was typed.
#[test]
fn both_names_run_the_same_program_and_speak_their_own_name() {
    let env = Env::new();
    env.file("ops/justfile", "deploy env:\n  echo {{env}}\n");
    let run = |bin: &str, args: &[&str]| {
        let o = env
            .cmd_for(bin)
            .args(args)
            .output()
            .expect("run the binary");
        (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    };
    let kiwa = env!("CARGO_BIN_EXE_kiwa");
    let (code, out, _) = run(kiwa, &["--version"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("kiwa "), "{out}");
    let (_, help, _) = run(kiwa, &["--help"]);
    assert!(
        help.contains("Usage: kiwa") && help.contains("  kiwa web dev"),
        "{help}"
    );
    let (code, _, err) = run(kiwa, &["ops", "deploy"]);
    assert_eq!(code, 2);
    assert!(err.contains("kiwa ops deploy -- <env>"), "{err}");
    let (_, _, err) = run(env!("CARGO_BIN_EXE_kw"), &["ops", "deploy"]);
    assert!(err.contains("kw ops deploy -- <env>"), "{err}");
}

#[test]
fn shell_init_names_the_picker_unless_it_would_shadow_a_command() {
    let env = Env::new();
    let o = env.kw(&["shell-init", "bash", "--name", "kpick"]);
    assert_eq!(o.code, 0, "{}", o.all());
    assert!(o.stdout.contains("kpick() { command kw \"$@\"; }"));
    let o = env.kw(&["shell-init", "zsh", "--name", "cd"]);
    assert_eq!(o.code, 2);
    assert!(o.stderr.contains("choose another name"), "{}", o.stderr);
}

#[test]
fn tasks_that_need_input_name_it_instead_of_running_bare() {
    let env = Env::new();
    env.file("ops/justfile", "deploy env:\n  echo deploying {{env}}\n");
    let j = env.project("ops");
    assert_eq!(
        j["tasks"][0]["params"],
        serde_json::json!([{ "name": "env", "form": "positional" }])
    );
    let o = env.kw(&["ops", "deploy"]);
    assert_eq!(o.code, 2, "{}", o.all());
    assert!(
        o.stderr
            .contains("`deploy` needs <env>: kw ops deploy -- <env>"),
        "{}",
        o.stderr
    );
    let j = env.kw(&["run", "ops", "deploy", "--json"]).json();
    assert_eq!(j["error"]["kind"], "missing-input");
    assert_eq!(
        env.kw(&["run", "ops", "deploy", "-n"]).code,
        0,
        "a dry run shows the command"
    );
}

#[test]
fn doctor_reports_terminal_files_and_runners() {
    let env = Env::new();
    let j = env.kw(&["doctor", "--json", "--no-mouse"]).json();
    assert_eq!(j["kind"], "doctor");
    let d = &j["data"];
    assert_eq!(d["terminal"]["stdout_tty"], false);
    assert_eq!(d["terminal"]["colours"]["enabled"], false, "NO_COLOR=1");
    assert_eq!(d["terminal"]["mouse"]["reason"], "--no-mouse");
    assert_eq!(d["config"]["exists"], false);
    assert_eq!(d["config"]["valid"], true);
    assert_eq!(d["state"]["runs"], 0);
    assert_eq!(d["runners"].as_array().unwrap().len(), 28);

    // A broken config is what doctor is for: it still reports, and names the problem.
    std::fs::write(env.config(), "roots {{{").unwrap();
    let o = env.kw(&["doctor"]);
    assert_eq!(o.code, 0, "{}", o.all());
    assert!(
        o.stdout.contains("✘") && o.stdout.contains("runners on PATH"),
        "{}",
        o.stdout
    );

    let p = env.kw(&["doctor", "--probe"]);
    assert_eq!(p.code, 2);
    assert!(p.stderr.contains("needs a terminal"), "{}", p.stderr);
}

#[test]
fn version_and_help() {
    let env = Env::new();
    let v = env.kw(&["--version"]);
    assert_eq!(v.code, 0);
    assert!(v.stdout.starts_with("kw "), "{}", v.stdout);
    let h = env.kw(&["--help"]);
    assert_eq!(h.code, 0);
    for needle in [
        "Examples:",
        "kw web dev",
        "--json",
        "KIWA_CONFIG",
        "NO_COLOR",
    ] {
        assert!(h.stdout.contains(needle), "help lacks {needle}");
    }
}

#[test]
fn bare_kw_without_terminal_lists_instead_of_failing() {
    let env = Env::new();
    node_project(&env, "web");
    let o = env.kw(&[]);
    assert_eq!(o.code, 0, "{}", o.all());
    assert!(o.stdout.contains("web"));
}

#[test]
fn no_color_means_no_escape_codes() {
    let env = Env::new();
    node_project(&env, "web");
    let o = env.kw(&["list", "--tasks"]);
    assert!(!o.all().contains('\u{1b}'), "escape codes found");
}

// ======================================================================== discovery

#[test]
fn list_json_envelope() {
    let env = Env::new();
    node_project(&env, "web");
    let j = env.kw(&["list", "--json"]).json();
    assert_eq!(j["schema"], "kiwa/v1");
    assert_eq!(j["ok"], true);
    assert_eq!(j["kind"], "project-list");
    assert_eq!(j["data"]["stats"]["projects"], 1);
    assert!(j["data"]["stats"]["scan_ms"].is_number());
    let t = &j["data"]["projects"][0]["tasks"][0];
    for k in ["name", "runner", "command"] {
        assert!(!t[k].is_null(), "task lacks {k}");
    }
}

#[test]
fn demo_workspace_has_every_ecosystem() {
    let env = Env::new();
    let d = demo();
    let j = env
        .kw(&["-w", d.to_str().unwrap(), "list", "--json"])
        .json();
    let projects = j["data"]["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 14);
    let mut eco: Vec<String> = projects
        .iter()
        .flat_map(|p| {
            p["ecosystems"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e.as_str().unwrap().to_owned())
        })
        .collect();
    eco.sort();
    eco.dedup();
    for e in [
        "node", "deno", "rust", "go", "python", "make", "just", "task", "mise", "gradle", "maven",
        "dotnet", "php", "ruby", "compose",
    ] {
        assert!(eco.contains(&e.to_string()), "missing ecosystem {e}");
    }
}

#[test]
fn nested_monorepo_packages_are_found() {
    let env = Env::new();
    env.file("shop/package.json", r#"{"scripts":{"dev":"turbo dev"}}"#)
        .file(
            "shop/apps/web/package.json",
            r#"{"scripts":{"dev":"vite"}}"#,
        )
        .file(
            "shop/packages/ui/package.json",
            r#"{"scripts":{"build":"tsc"}}"#,
        );
    let ids = env.ids(&[]);
    assert_eq!(ids, ["shop", "shop/apps/web", "shop/packages/ui"]);
}

#[test]
fn build_and_dependency_folders_are_skipped() {
    let env = Env::new();
    env.file("app/package.json", "{}");
    for d in [
        "node_modules/x",
        "target/x",
        "vendor/x",
        "dist/x",
        ".hidden/x",
        "__pycache__/x",
    ] {
        env.file(&format!("app/{d}/package.json"), "{}");
    }
    assert_eq!(env.ids(&[]), ["app"]);
}

#[test]
fn folders_named_like_build_output_are_projects_outside_a_project() {
    let env = Env::new();
    for n in ["bin", "build", "out", "dist", "target", "obj"] {
        env.file(&format!("{n}/go.mod"), "module x");
    }
    env.file("app/go.mod", "module app")
        .file("app/bin/go.mod", "module generated");
    let ids = env.ids(&[]);
    for n in ["bin", "build", "out", "dist", "target", "obj", "app"] {
        assert!(ids.contains(&n.to_string()), "{n} missing: {ids:?}");
    }
    assert!(
        !ids.contains(&"app/bin".to_string()),
        "output dir inside a project is skipped"
    );
}

#[test]
fn gitignored_folders_are_skipped() {
    let env = Env::new();
    env.dir(".git").file(".gitignore", "generated/\n");
    env.file("app/go.mod", "module app")
        .file("generated/go.mod", "module gen");
    let ids = env.ids(&[]);
    assert!(ids.contains(&"app".to_string()));
    assert!(!ids.iter().any(|i| i.starts_with("generated")), "{ids:?}");
}

#[test]
fn depth_flag_limits_the_scan() {
    let env = Env::new();
    env.file("a/go.mod", "module a")
        .file("x/y/z/go.mod", "module z");
    assert_eq!(env.ids(&["--depth", "1"]), ["a"]);
    assert_eq!(env.ids(&["--depth", "3"]), ["a", "x/y/z"]);
}

#[test]
fn several_roots() {
    let env = Env::new();
    env.file("one/a/go.mod", "module a")
        .file("two/b/go.mod", "module b");
    let one = env.ws().join("one");
    let two = env.ws().join("two");
    let ids = env.ids(&["-w", one.to_str().unwrap(), "-w", two.to_str().unwrap()]);
    assert_eq!(ids, ["a", "b"]);
}

#[test]
fn git_repo_without_manifest_is_listed_without_tasks() {
    let env = Env::new();
    env.file("notes/.git/HEAD", "ref: refs/heads/main\n")
        .file("notes/README.md", "x");
    let p = env.project("notes");
    assert_eq!(p["ecosystems"][0], "git");
    assert_eq!(p["branch"], "main");
    assert!(p["tasks"].as_array().unwrap().is_empty());
}

#[test]
fn git_branch_detached_and_worktree() {
    let env = Env::new();
    env.file("a/go.mod", "module a")
        .file("a/.git/HEAD", "0123456789abcdef0123456789abcdef01234567\n");
    assert_eq!(env.project("a")["branch"], "0123456");
    let gitdir = env.ws().join("wtdata");
    std::fs::create_dir_all(&gitdir).unwrap();
    std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/feature/x\n").unwrap();
    env.file("b/go.mod", "module b")
        .file("b/.git", &format!("gitdir: {}\n", gitdir.display()));
    assert_eq!(env.project("b")["branch"], "feature/x");
}

#[test]
fn natural_sort_order() {
    let env = Env::new();
    for n in ["app10", "App2", "app1", "beta"] {
        env.file(&format!("{n}/go.mod"), "module x");
    }
    assert_eq!(env.ids(&[]), ["app1", "App2", "app10", "beta"]);
}

#[test]
fn ids_use_forward_slashes_on_every_os() {
    let env = Env::new();
    env.file("a/b/go.mod", "module b");
    assert_eq!(env.ids(&[]), ["a/b"]);
}

#[test]
fn linked_project_folders_are_followed() {
    let env = Env::new();
    let target = env.dir.path().join("outside");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("go.mod"), "module linked").unwrap();
    let link = env.ws().join("linked");
    #[cfg(windows)]
    let ok = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .is_ok_and(|o| o.status.success());
    #[cfg(unix)]
    let ok = std::os::unix::fs::symlink(&target, &link).is_ok();
    assert!(ok, "could not create link");
    assert_eq!(env.ids(&[]), ["linked"]);
}

// ======================================================================== JS specifics

#[test]
fn bom_in_package_json_is_accepted() {
    let env = Env::new();
    env.file("a/package.json", "\u{feff}{\"scripts\":{\"dev\":\"vite\"}}");
    let p = env.project("a");
    assert_eq!(task_names(&p), ["dev"]);
    assert!(p["warnings"].is_null());
}

#[test]
fn broken_package_json_is_listed_with_a_warning() {
    let env = Env::new();
    env.file("a/package.json", "{ \"scripts\": { ");
    let p = env.project("a");
    assert!(p["warnings"][0].as_str().unwrap().contains("invalid JSON"));
    let o = env.kw(&["a", "dev"]);
    assert_eq!(o.code, 3, "{}", o.all());
    let human = env.kw(&["list"]);
    assert!(
        human.stdout.contains("invalid JSON"),
        "warning not shown: {}",
        human.stdout
    );
}

#[test]
fn scripts_of_the_wrong_type_warn() {
    let env = Env::new();
    env.file("a/package.json", r#"{"scripts":["x"]}"#);
    assert!(
        env.project("a")["warnings"][0]
            .as_str()
            .unwrap()
            .contains("must be an object")
    );
}

#[test]
fn package_manager_detection() {
    let env = Env::new();
    let cases = [
        ("npm1", "package-lock.json", "npm"),
        ("npm2", "npm-shrinkwrap.json", "npm"),
        ("pnpm", "pnpm-lock.yaml", "pnpm"),
        ("yarn", "yarn.lock", "yarn"),
        ("bun1", "bun.lock", "bun"),
        ("bun2", "bun.lockb", "bun"),
    ];
    for (dir, lock, _) in cases {
        env.file(&format!("{dir}/package.json"), r#"{"scripts":{"x":"y"}}"#)
            .file(&format!("{dir}/{lock}"), "");
    }
    env.file(
        "field/package.json",
        r#"{"packageManager":"bun@1.2.0","scripts":{"x":"y"}}"#,
    )
    .file("field/package-lock.json", "{}")
    .file("none/package.json", r#"{"scripts":{"x":"y"}}"#);
    for (dir, _, pm) in cases {
        assert_eq!(env.project(dir)["tasks"][0]["runner"], pm, "{dir}");
    }
    assert_eq!(
        env.project("field")["tasks"][0]["runner"],
        "bun",
        "packageManager wins"
    );
    assert_eq!(env.project("none")["tasks"][0]["runner"], "npm", "default");
}

#[test]
fn monorepo_package_uses_the_root_lockfile() {
    let env = Env::new();
    env.file("mono/pnpm-lock.yaml", "")
        .file("mono/package.json", "{}")
        .file(
            "mono/apps/web/package.json",
            r#"{"scripts":{"dev":"vite"}}"#,
        );
    assert_eq!(env.project("mono/apps/web")["tasks"][0]["runner"], "pnpm");
}

const HOOKS: &str = r#"{"scripts":{"prebuild":"a","build":"b","postbuild":"c","postinstall":"d"}"#;

#[test]
fn npm_hooks_are_folded_into_their_script() {
    let env = Env::new();
    env.file("a/package.json", &format!("{HOOKS}}}"))
        .file("a/package-lock.json", "{}");
    let p = env.project("a");
    assert_eq!(task_names(&p), ["build", "postinstall"]);
    assert_eq!(
        p["tasks"][0]["hooks"],
        serde_json::json!(["prebuild", "postbuild"])
    );
}

#[test]
fn yarn_berry_keeps_hooks_runnable() {
    let env = Env::new();
    env.file("a/package.json", &format!("{HOOKS}}}"))
        .file("a/yarn.lock", "# generated\n\n__metadata:\n  version: 8\n");
    assert_eq!(task_names(&env.project("a")).len(), 4);
    env.file(
        "b/package.json",
        &format!("{HOOKS},\"packageManager\":\"yarn@4.5.3\"}}"),
    );
    assert_eq!(task_names(&env.project("b")).len(), 4);
    env.file(
        "c/package.json",
        &format!("{HOOKS},\"packageManager\":\"yarn@1.22.22\"}}"),
    );
    assert_eq!(task_names(&env.project("c")).len(), 2, "Yarn 1 runs hooks");
}

#[test]
fn pnpm_hooks_follow_version_and_npmrc() {
    let env = Env::new();
    env.file(
        "p8/package.json",
        &format!("{HOOKS},\"packageManager\":\"pnpm@8.15.0\"}}"),
    );
    env.file(
        "p9/package.json",
        &format!("{HOOKS},\"packageManager\":\"pnpm@9.0.5\"}}"),
    );
    env.file(
        "rc/package.json",
        &format!("{HOOKS},\"packageManager\":\"pnpm@9.0.5\"}}"),
    )
    .file("rc/.npmrc", "enable-pre-post-scripts=false\n");
    assert_eq!(
        task_names(&env.project("p8")).len(),
        4,
        "pnpm 8 does not run hooks by default"
    );
    assert_eq!(task_names(&env.project("p9")).len(), 2, "pnpm 9 does");
    assert_eq!(task_names(&env.project("rc")).len(), 4, ".npmrc wins");
}

#[test]
fn missing_install_is_hinted() {
    let env = Env::new();
    env.file(
        "a/package.json",
        r#"{"dependencies":{"x":"1"},"scripts":{"x":"y"}}"#,
    )
    .file("a/pnpm-lock.yaml", "");
    let p = env.project("a");
    assert!(
        p["hints"][0].as_str().unwrap().contains("pnpm install"),
        "{p}"
    );
    env.dir("a/node_modules");
    assert!(env.project("a")["hints"].is_null());
}

// ======================================================================== other ecosystems

#[test]
fn cargo_workspace_and_aliases() {
    let env = Env::new();
    env.file("ws/Cargo.toml", "[workspace]\nmembers=[\"x\"]\n")
        .file(
            "ws/.cargo/config.toml",
            "[alias]\nxtask = \"run -p xtask --\"\n",
        )
        .file("bin/Cargo.toml", "[package]\nname=\"b\"\n")
        .file("bin/src/main.rs", "fn main(){}");
    let ws = env.project("ws");
    assert_eq!(
        ws["tasks"][0]["command"],
        serde_json::json!(["cargo", "build", "--workspace"])
    );
    assert!(task_names(&ws).contains(&"xtask".to_string()));
    assert!(task_names(&env.project("bin")).contains(&"run".to_string()));
}

#[test]
fn python_runners() {
    let env = Env::new();
    env.file(
        "u/pyproject.toml",
        "[project]\nname=\"u\"\n[project.scripts]\ncli=\"u:main\"\n",
    )
    .file("u/uv.lock", "")
    .dir("u/tests");
    env.file(
        "p/pyproject.toml",
        "[tool.poetry]\nname=\"p\"\n[tool.poe.tasks]\nlint=\"ruff\"\n",
    );
    env.file(
        "d/pyproject.toml",
        "[tool.pdm.scripts]\nstart=\"python -m d\"\n",
    );
    env.file("r/requirements.txt", "requests\n");
    let u = env.project("u");
    assert_eq!(
        u["tasks"][0]["command"],
        serde_json::json!(["uv", "run", "cli"])
    );
    assert!(task_names(&u).contains(&"test".to_string()));
    assert_eq!(
        env.project("p")["tasks"][0]["command"],
        serde_json::json!(["poetry", "run", "poe", "lint"])
    );
    assert_eq!(
        env.project("d")["tasks"][0]["command"],
        serde_json::json!(["pdm", "run", "start"])
    );
    assert_eq!(env.project("r")["ecosystems"][0], "python");
}

#[test]
fn generic_runners_are_parsed_statically() {
    let env = Env::new();
    env.file(
        "m/Makefile",
        "VAR := 1\n.PHONY: all\nall: build ## Build all\n\techo\nbuild:\n\techo\n",
    )
    .file(
        "j/justfile",
        "# Run tests\ntest:\n  echo\n_hidden:\n  echo\n",
    )
    .file(
        "t/Taskfile.yml",
        "version: '3'\ntasks:\n  lint:\n    desc: Lint it\n    cmds: [x]\n",
    )
    .file(
        "s/mise.toml",
        "[tasks.fmt]\ndescription = \"Format\"\nrun = \"x\"\n",
    );
    assert_eq!(task_names(&env.project("m")), ["all", "build"]);
    assert_eq!(env.project("m")["tasks"][0]["description"], "Build all");
    assert_eq!(task_names(&env.project("j")), ["test"]);
    assert_eq!(env.project("t")["tasks"][0]["description"], "Lint it");
    assert_eq!(
        env.project("s")["tasks"][0]["command"],
        serde_json::json!(["mise", "run", "fmt"])
    );
}

#[test]
fn jvm_dotnet_php_ruby_deno_compose() {
    let env = Env::new();
    env.file("g/build.gradle.kts", "")
        .file("g/gradlew", "")
        .file("g/gradlew.bat", "");
    env.file("mv/pom.xml", "<project/>");
    env.file("n/App.csproj", "<Project/>");
    env.file("ph/composer.json", r#"{"scripts":{"test":"phpunit"}}"#);
    env.file("rb/Rakefile", "task :spec\n")
        .file("rb/Gemfile", "");
    env.file("de/deno.json", r#"{"tasks":{"dev":"deno run main.ts"}}"#);
    env.file("dc/compose.yaml", "services: {}\n");
    let g = env.project("g");
    assert!(
        g["tasks"][0]["command"][0]
            .as_str()
            .unwrap()
            .contains("gradlew"),
        "wrapper preferred"
    );
    assert_eq!(env.project("mv")["tasks"][0]["command"][0], "mvn");
    assert!(task_names(&env.project("n")).contains(&"run".to_string()));
    assert_eq!(
        env.project("ph")["tasks"][1]["command"],
        serde_json::json!(["composer", "run-script", "test"])
    );
    assert_eq!(
        env.project("rb")["tasks"][1]["command"],
        serde_json::json!(["bundle", "exec", "rake", "spec"])
    );
    assert_eq!(
        env.project("de")["tasks"][0]["command"],
        serde_json::json!(["deno", "task", "dev"])
    );
    assert_eq!(env.project("dc")["tasks"][0]["name"], "compose:up");
}

#[test]
fn discovery_never_executes_project_code() {
    // A Makefile/justfile/Rakefile that would create a file if executed.
    let env = Env::new();
    let marker = env.dir.path().join("PWNED");
    let m = marker.display().to_string().replace('\\', "/");
    env.file(
        "evil/Makefile",
        &format!("$(shell touch {m})\nall:\n\ttouch {m}\n"),
    )
    .file(
        "evil/justfile",
        &format!("x := `touch {m}`\nall:\n  touch {m}\n"),
    )
    .file(
        "evil/Rakefile",
        &format!("File.write('{m}', 'x')\ntask :all\n"),
    )
    .file(
        "evil/build.gradle",
        &format!("new File('{m}').text = 'x'\n"),
    );
    let o = env.kw(&["list", "--tasks", "--json"]);
    assert_eq!(o.code, 0);
    assert!(!marker.exists(), "discovery executed project code");
}

// ======================================================================== resolution

#[test]
fn project_resolution_order() {
    let env = Env::new();
    for n in ["api", "api-v2", "api-gateway", "Web"] {
        env.file(&format!("{n}/package.json"), r#"{"scripts":{"start":"x"}}"#);
    }
    let dry = |p: &str| env.kw(&["run", p, "start", "-n", "--json"]);
    assert_eq!(
        dry("api").json()["data"]["project"],
        "api",
        "exact wins over prefix"
    );
    assert_eq!(
        dry("web").json()["data"]["project"],
        "Web",
        "case-insensitive"
    );
    assert_eq!(
        dry("api-v").json()["data"]["project"],
        "api-v2",
        "unique prefix"
    );
    let amb = dry("ap");
    assert_eq!(amb.code, 4);
    let j = amb.json();
    assert_eq!(j["ok"], false);
    assert_eq!(j["error"]["kind"], "ambiguous");
    assert_eq!(j["error"]["suggestions"].as_array().unwrap().len(), 3);
}

#[test]
fn not_found_suggests_and_never_runs_a_guess() {
    let env = Env::new();
    env.file("hello/package.json", r#"{"scripts":{"greet":"x"}}"#);
    let o = env.kw(&["run", "helo", "greet", "--json"]);
    assert_eq!(o.code, 3);
    let j = o.json();
    assert_eq!(j["error"]["kind"], "not-found");
    assert_eq!(j["error"]["suggestions"][0], "hello");
    let human = env.kw(&["helo", "greet"]);
    assert!(
        human.stderr.contains("did you mean: hello"),
        "{}",
        human.stderr
    );
}

#[test]
fn task_resolution_and_runner_prefix() {
    let env = Env::new();
    env.file(
        "a/package.json",
        r#"{"scripts":{"build":"x","bundle":"y"}}"#,
    )
    .file("a/Makefile", "build:\n\techo\n");
    let amb = env.kw(&["run", "a", "build", "-n", "--json"]);
    assert_eq!(amb.code, 4, "npm and make both define build");
    assert!(
        amb.json()["error"]["suggestions"]
            .to_string()
            .contains("make:build")
    );
    let j = env.kw(&["run", "a", "make:build", "-n", "--json"]).json();
    assert_eq!(j["data"]["argv"], serde_json::json!(["make", "build"]));
    assert_eq!(env.kw(&["run", "a", "bu", "-n"]).code, 4, "bu is ambiguous");
    assert_eq!(env.kw(&["run", "a", "bun", "-n"]).code, 0, "bun → bundle");
    let missing = env.kw(&["run", "a", "deploy", "--json"]);
    assert_eq!(missing.code, 3);
}

#[test]
fn missing_task_without_terminal_lists_tasks() {
    let env = Env::new();
    env.file("a/package.json", r#"{"scripts":{"dev":"x","test":"y"}}"#);
    let o = env.kw(&["a"]);
    assert_eq!(o.code, 2);
    assert!(o.all().contains("dev") && o.all().contains("test"));
}

#[test]
fn extra_positionals_need_a_separator() {
    let env = Env::new();
    env.file("a/package.json", r#"{"scripts":{"dev":"x"}}"#);
    let o = env.kw(&["a", "dev", "--port"]);
    assert_eq!(o.code, 2);
    assert!(o.stderr.contains("after --"), "{}", o.stderr);
}

// ======================================================================== argv & separators

fn argv(env: &Env, project: &str, task: &str) -> Value {
    env.kw(&["run", project, task, "-n", "--json", "--", "-x", "a b"])
        .json()["data"]["argv"]
        .clone()
}

#[test]
fn forwarded_args_get_the_separator_each_runner_needs() {
    let env = Env::new();
    let s = r#""scripts":{"t":"x"}"#;
    env.file("npm/package.json", &format!("{{{s}}}"));
    env.file(
        "pnpm/package.json",
        &format!("{{{s},\"packageManager\":\"pnpm@9.0.5\"}}"),
    );
    env.file(
        "y1/package.json",
        &format!("{{{s},\"packageManager\":\"yarn@1.22.22\"}}"),
    );
    env.file(
        "y4/package.json",
        &format!("{{{s},\"packageManager\":\"yarn@4.5.3\"}}"),
    );
    env.file(
        "bun/package.json",
        &format!("{{{s},\"packageManager\":\"bun@1.3.0\"}}"),
    );
    env.file("php/composer.json", r#"{"scripts":{"t":"phpunit"}}"#);
    let j = |v: &[&str]| serde_json::json!(v);
    assert_eq!(
        argv(&env, "npm", "t"),
        j(&["npm", "run", "t", "--", "-x", "a b"])
    );
    assert_eq!(
        argv(&env, "pnpm", "t"),
        j(&["pnpm", "run", "t", "-x", "a b"])
    );
    assert_eq!(
        argv(&env, "y1", "t"),
        j(&["yarn", "run", "t", "--", "-x", "a b"])
    );
    assert_eq!(argv(&env, "y4", "t"), j(&["yarn", "run", "t", "-x", "a b"]));
    assert_eq!(argv(&env, "bun", "t"), j(&["bun", "run", "t", "-x", "a b"]));
    assert_eq!(
        argv(&env, "php", "t"),
        j(&["composer", "run-script", "t", "--", "-x", "a b"])
    );
}

// ======================================================================== running (needs npm)

#[test]
fn exit_code_is_mirrored() {
    if !has_npm() {
        return;
    }
    let env = Env::new();
    node_project(&env, "a");
    assert_eq!(env.kw(&["a", "ok"]).code, 0);
    assert_eq!(env.kw(&["a", "fail"]).code, 3);
}

#[test]
fn arguments_reach_the_script_intact() {
    if !has_npm() {
        return;
    }
    let env = Env::new();
    node_project(&env, "a");
    let o = env.kw(&["a", "show", "--", "a b", "c\"d", "-x", "--y", "--", "z"]);
    assert!(
        o.all()
            .contains(r#"ARGS=["a b","c\"d","-x","--y","--","z"]"#),
        "{}",
        o.all()
    );
}

#[test]
fn shell_metacharacters_are_data() {
    if !has_npm() {
        return;
    }
    let env = Env::new();
    node_project(&env, "a");
    let marker = env.dir.path().join("INJECTED");
    let m = marker.display().to_string();
    let payload = if cfg!(windows) {
        format!("& echo x > \"{m}\"")
    } else {
        format!("; touch '{m}'")
    };
    let o = env.kw(&["a", "show", "--", &payload]);
    assert_eq!(o.code, 0, "{}", o.all());
    assert!(!marker.exists(), "argument was executed by a shell");
}

#[test]
fn json_run_keeps_stdout_clean() {
    if !has_npm() {
        return;
    }
    let env = Env::new();
    node_project(&env, "a");
    let o = env.kw(&["run", "a", "ok", "--json"]);
    let j = o.json(); // panics if task output leaked to stdout
    assert_eq!(j["kind"], "run");
    assert_eq!(j["data"]["exit_code"], 0);
    assert!(o.stderr.contains("RAN-OK"));
}

#[test]
fn folder_with_spaces_and_accents() {
    if !has_npm() {
        return;
    }
    let env = Env::new();
    node_project(&env, "mon projet é");
    let o = env.kw(&["mon projet é", "ok"]);
    assert_eq!(o.code, 0, "{}", o.all());
    assert!(o.all().contains("RAN-OK"));
}

#[test]
fn history_and_last() {
    if !has_npm() {
        return;
    }
    let env = Env::new();
    node_project(&env, "a");
    assert_eq!(env.kw(&["last"]).code, 3, "nothing run yet");
    env.kw(&["a", "ok"]);
    let j = env.kw(&["last", "-n", "--json"]).json();
    assert_eq!(j["data"]["task"], "ok");
    let h = env.kw(&["history", "--json"]).json();
    assert_eq!(h["data"][0]["count"], 1);
    assert_eq!(h["data"][0]["exit_code"], 0);
    env.kw(&["history", "--clear"]);
    assert!(
        env.kw(&["history", "--json"]).json()["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn missing_runner_exits_127_with_an_install_hint() {
    let env = Env::new();
    env.file("a/package.json", r#"{"scripts":{"x":"y"}}"#);
    let o = env
        .cmd()
        .args(["run", "a", "x", "--json"])
        .env("PATH", env.dir.path().join("empty-path"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(127));
    let j: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(j["error"]["kind"], "runner-missing");
    assert!(
        j["error"]["suggestions"][0]
            .as_str()
            .unwrap()
            .contains("nodejs.org")
    );
}

// ======================================================================== config

#[test]
fn config_set_show_and_validation() {
    let env = Env::new();
    std::fs::write(env.config(), "// keep me\nroots \"/nowhere\"\n").unwrap();
    assert_eq!(env.kw(&["config", "set", "accent", "amber"]).code, 0);
    assert_eq!(env.kw(&["config", "set", "mouse", "off"]).code, 0);
    assert_eq!(env.kw(&["config", "set", "depth", "2"]).code, 0);
    assert_eq!(env.kw(&["config", "set", "accent", "green"]).code, 2);
    assert_eq!(env.kw(&["config", "set", "nope", "1"]).code, 2);
    let text = std::fs::read_to_string(env.config()).unwrap();
    assert!(text.contains("// keep me"));
    let j = env.kw(&["config", "show", "--json"]).json();
    assert_eq!(j["data"]["accent"], "amber");
    assert_eq!(j["data"]["mouse"], false);
    assert_eq!(j["data"]["depth"], 2);
    let p = env.kw(&["config", "path", "--json"]).json();
    assert_eq!(p["data"].as_str().unwrap(), env.config().to_str().unwrap());
}

#[test]
fn config_roots_are_used_when_no_flag() {
    let env = Env::new();
    env.file("elsewhere/a/go.mod", "module a")
        .file("here/go.mod", "module h");
    let root = env
        .ws()
        .join("elsewhere")
        .display()
        .to_string()
        .replace('\\', "/");
    std::fs::write(env.config(), format!("roots \"{root}\"\n")).unwrap();
    assert_eq!(env.ids(&[]), ["a"]);
}

#[test]
fn invalid_config_is_exit_5_and_unknown_keys_only_warn() {
    let env = Env::new();
    std::fs::write(env.config(), "roots \"unclosed\n").unwrap();
    let o = env.kw(&["list", "--json"]);
    assert_eq!(o.code, 5);
    assert_eq!(o.json()["error"]["kind"], "config");
    std::fs::write(env.config(), "colour \"x\"\n").unwrap();
    let o = env.kw(&["list"]);
    assert_eq!(o.code, 0);
    assert!(o.stderr.contains("unknown config node"));
}

// ======================================================================== aliases

#[test]
fn alias_lifecycle() {
    let env = Env::new();
    env.file("api/package.json", r#"{"scripts":{"test":"x"}}"#);
    assert_eq!(
        env.kw(&["alias", "add", "t", "api", "test", "--", "--watch"])
            .code,
        0
    );
    let l = env.kw(&["alias", "list", "--json"]).json();
    assert_eq!(l["data"][0]["name"], "t");
    assert_eq!(l["data"][0]["args"], serde_json::json!(["--watch"]));
    assert_eq!(
        env.kw(&["alias", "add", "t", "api", "test"]).code,
        0,
        "replace"
    );
    assert_eq!(
        env.kw(&["alias", "list", "--json"]).json()["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        env.kw(&["alias", "add", "list", "api", "test"]).code,
        2,
        "reserved"
    );
    assert_eq!(
        env.kw(&["alias", "add", "a b", "api", "test"]).code,
        2,
        "invalid name"
    );
    assert_eq!(env.kw(&["alias", "rm", "t"]).code, 0);
    assert_eq!(env.kw(&["alias", "rm", "t"]).code, 3);
}

#[test]
fn alias_runs_its_task() {
    if !has_npm() {
        return;
    }
    let env = Env::new();
    node_project(&env, "a");
    env.kw(&["alias", "add", "s", "a", "show", "--", "preset"]);
    let o = env.kw(&["s", "--", "extra"]);
    assert!(
        o.all().contains(r#"ARGS=["preset","extra"]"#),
        "{}",
        o.all()
    );
}

// ======================================================================== init

#[test]
fn init_with_folders_writes_config_and_keeps_comments() {
    let env = Env::new();
    env.file("dev/a/go.mod", "module a");
    std::fs::write(env.config(), "// mine\nalias \"t\" \"a\" \"test\"\n").unwrap();
    let dev = env.ws().join("dev");
    let o = env.kw(&[
        "init",
        "--use",
        dev.to_str().unwrap(),
        "--accent",
        "violet",
        "--json",
    ]);
    assert_eq!(o.code, 0, "{}", o.all());
    let text = std::fs::read_to_string(env.config()).unwrap();
    assert!(text.contains("// mine") && text.contains("alias"));
    let j = env.kw(&["config", "show", "--json"]).json();
    assert_eq!(j["data"]["accent"], "violet");
    assert_eq!(env.ids(&[]), ["a"]);
}

#[test]
fn init_rejects_bad_input() {
    let env = Env::new();
    assert_eq!(env.kw(&["init", "--use", "/definitely/not/here"]).code, 2);
    let ws = env.ws();
    assert_eq!(
        env.kw(&["init", "--use", ws.to_str().unwrap(), "--accent", "red"])
            .code,
        2
    );
}

#[test]
fn init_without_terminal_returns_suggestions_with_counts() {
    let env = Env::new();
    let home = env.dir.path().join("home");
    std::fs::create_dir_all(home.join("dev/x")).unwrap();
    std::fs::write(home.join("dev/x/go.mod"), "module x").unwrap();
    let j = env.kw(&["init", "--json"]).json();
    assert_eq!(j["kind"], "init-suggestions");
    let s = j["data"].as_array().unwrap();
    let dev = s
        .iter()
        .find(|x| {
            x["path"]
                .as_str()
                .unwrap()
                .replace('\\', "/")
                .ends_with("home/dev")
        })
        .expect("~/dev suggested");
    assert_eq!(dev["projects"], 1);
}

// ======================================================================== shell integration

#[test]
fn completions_for_every_shell() {
    let env = Env::new();
    for sh in [
        "bash",
        "zsh",
        "fish",
        "powershell",
        "pwsh",
        "elvish",
        "nushell",
        "nu",
    ] {
        let o = env.kw(&["completions", sh]);
        assert_eq!(o.code, 0, "{sh}");
        assert!(o.stdout.len() > 500, "{sh} script too short");
        assert!(o.stdout.contains("kw"), "{sh}");
    }
    assert_eq!(env.kw(&["completions", "cmd"]).code, 2);
}

#[test]
fn shell_init_defines_kcd_and_alias_commands() {
    let env = Env::new();
    std::fs::write(env.config(), "alias \"t\" \"api\" \"test\"\n").unwrap();
    let bash = env.kw(&["shell-init", "bash"]).stdout;
    assert!(bash.contains("kcd()") && bash.contains("kw path") && bash.contains("t() {"));
    let ps = env.kw(&["shell-init", "powershell"]).stdout;
    assert!(
        ps.contains("function kcd") && ps.contains("Set-Location") && ps.contains("function t")
    );
    let fish = env.kw(&["shell-init", "fish"]).stdout;
    assert!(fish.contains("function kcd"));
}

#[test]
fn path_prints_the_project_directory() {
    let env = Env::new();
    env.file("deep/app/go.mod", "module a");
    let o = env.kw(&["path", "app"]);
    assert_eq!(o.code, 0);
    let printed = PathBuf::from(o.stdout.trim());
    assert!(
        printed.ends_with(Path::new("deep").join("app")),
        "{}",
        o.stdout
    );
    assert_eq!(env.kw(&["path", "zzz"]).code, 3);
}

// ======================================================================== reset

#[test]
fn reset_needs_confirmation_and_removes_files() {
    let env = Env::new();
    std::fs::write(env.config(), "accent \"red\"\n").unwrap();
    std::fs::write(env.state(), "{\"history\":[]}").unwrap();
    let o = env.kw(&["reset"]);
    assert_eq!(o.code, 2, "no terminal, no --yes");
    assert!(env.config().exists());
    let o = env.kw(&["reset", "--yes", "--json"]);
    assert_eq!(o.code, 0);
    assert!(!env.config().exists() && !env.state().exists());
}

// ======================================================================== config v1, workspaces, hide

#[test]
fn config_is_versioned_backed_up_and_bom_tolerant() {
    let env = Env::new();
    std::fs::write(env.config(), "\u{feff}// mine\naccent \"rose\"\n").unwrap();
    assert_eq!(
        env.kw(&["config", "get", "accent"]).stdout.trim(),
        "rose",
        "BOM accepted"
    );
    assert_eq!(env.kw(&["config", "set", "mouse", "off"]).code, 0);
    let text = std::fs::read_to_string(env.config()).unwrap();
    assert!(
        text.contains("version 1") && text.contains("// mine"),
        "{text}"
    );
    let bak = std::fs::read_to_string(env.dir.path().join("config.kdl.bak")).unwrap();
    assert!(
        bak.contains("accent \"rose\"") && !bak.contains("mouse"),
        "backup is the previous file"
    );
    std::fs::write(env.config(), "version 2\n").unwrap();
    let o = env.kw(&["list", "--json"]);
    assert_eq!(o.code, 5);
    assert!(
        o.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Upgrade kiwa")
    );
    std::fs::write(env.config(), "version \"x\"\n").unwrap();
    assert_eq!(env.kw(&["list"]).code, 5);
}

#[test]
fn config_get_and_validation() {
    let env = Env::new();
    std::fs::write(env.config(), "roots \"/a\" \"/b\"\nhide \"*:lint\"\n").unwrap();
    assert_eq!(
        env.kw(&["config", "get", "depth"]).stdout.trim(),
        "4",
        "default value"
    );
    assert_eq!(
        env.kw(&["config", "get", "roots"]).stdout.lines().count(),
        2
    );
    let j = env.kw(&["config", "get", "hide", "--json"]).json();
    assert_eq!(j["data"]["value"][0], "*:lint");
    assert_eq!(env.kw(&["config", "get", "nope"]).code, 2);
    assert_eq!(env.kw(&["config", "edit"]).code, 2, "edit needs a terminal");
}

#[test]
fn hide_patterns_and_git_only_repositories() {
    let env = Env::new();
    env.file(
        "web/package.json",
        r#"{"scripts":{"dev":"x","lint":"y","lint:fix":"z"}}"#,
    )
    .file("notes/.git/HEAD", "ref: refs/heads/main\n");
    std::fs::write(
        env.config(),
        "hide \"web:lint*\" \"npm:dev\"\nshow-git-only #false\n",
    )
    .unwrap();
    let ids = env.ids(&[]);
    assert_eq!(ids, ["web"], "git-only repository hidden");
    assert!(
        env.project("web")["tasks"].as_array().unwrap().is_empty(),
        "all three hidden"
    );
    std::fs::write(env.config(), "hide \"*:lint:*\"\n").unwrap();
    let names = task_names(&env.project("web"));
    assert_eq!(names, ["dev", "lint"]);
    assert_eq!(env.ids(&[]), ["notes", "web"]);
}

#[test]
fn named_workspaces() {
    let env = Env::new();
    env.file("work/api/go.mod", "module a")
        .file("perso/blog/go.mod", "module b")
        .file("deep/x/y/z/go.mod", "module z");
    let w = env
        .ws()
        .join("work")
        .display()
        .to_string()
        .replace('\\', "/");
    let p = env
        .ws()
        .join("perso")
        .display()
        .to_string()
        .replace('\\', "/");
    let d = env
        .ws()
        .join("deep")
        .display()
        .to_string()
        .replace('\\', "/");
    std::fs::write(
        env.config(),
        format!("roots \"{p}\"\nworkspace \"work\" {{\n    roots \"{w}\"\n}}\nworkspace \"deep\" {{\n    roots \"{d}\"\n    depth 1\n}}\n"),
    )
    .unwrap();
    assert_eq!(env.ids(&[]), ["blog"], "top-level roots by default");
    assert_eq!(env.ids(&["-W", "work"]), ["api"]);
    assert!(
        env.ids(&["-W", "deep"]).is_empty(),
        "workspace depth applies"
    );
    let o = env
        .cmd()
        .args(["list", "--json"])
        .env("KIWA_WORKSPACE", "work")
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("\"api\""),
        "KIWA_WORKSPACE"
    );
    let bad = env.kw(&["-W", "wrok", "list", "--json"]);
    assert_eq!(bad.code, 3);
    assert!(
        bad.json()["error"]["suggestions"]
            .to_string()
            .contains("work")
    );
    let ws = env.kw(&["config", "get", "workspaces", "--json"]).json();
    assert_eq!(ws["data"]["value"].as_array().unwrap().len(), 2);
}

#[test]
fn json_says_why_each_ecosystem_was_detected() {
    let env = Env::new();
    env.file("mono/pnpm-lock.yaml", "")
        .file("mono/package.json", "{}");
    env.file("mono/apps/web/package.json", r#"{"scripts":{"dev":"x"}}"#);
    env.file("pm/package.json", r#"{"packageManager":"bun@1.2.0"}"#);
    env.file("none/package.json", "{}")
        .file("none/Makefile", "x:\n");
    let from = |id: &str| env.project(id)["detected_from"].clone();
    assert_eq!(from("mono/apps/web")[0]["from"], "../../pnpm-lock.yaml");
    assert_eq!(from("pm")[0]["from"], "packageManager: bun@1.2.0");
    assert_eq!(from("none")[0]["from"], "no lockfile (npm by default)");
    assert_eq!(from("none")[1]["ecosystem"], "make");
    assert_eq!(from("none")[1]["from"], "Makefile");
}

// ======================================================================== no child left behind

fn alive(pid: u32) -> bool {
    if cfg!(windows) {
        let o = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).contains(&pid.to_string())
    } else {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success())
    }
}

#[test]
fn killing_kw_also_stops_the_task() {
    if !has_npm() {
        return;
    }
    let env = Env::new();
    let pid_file = env.dir.path().join("task.pid");
    let pf = pid_file.display().to_string().replace('\\', "/");
    env.file("a/package-lock.json", "{}")
        .file("a/package.json", r#"{"scripts":{"serve":"node keep.js"}}"#)
        .file(
            "a/keep.js",
            &format!("require('fs').writeFileSync('{pf}', String(process.pid)); setInterval(() => {{}}, 1000);"),
        );
    let mut kw = env.cmd().args(["a", "serve"]).spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !pid_file.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the task never started"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let task: u32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(alive(task));
    if cfg!(windows) {
        kw.kill().unwrap(); // TerminateProcess, as Task Manager or `taskkill /f` do
    } else {
        Command::new("kill")
            .args(["-TERM", &kw.id().to_string()])
            .status()
            .unwrap();
    }
    let _ = kw.wait();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while alive(task) {
        assert!(
            std::time::Instant::now() < deadline,
            "task {task} survived kw"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
