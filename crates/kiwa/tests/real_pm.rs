// SPDX-License-Identifier: MIT OR Apache-2.0
//! Real package managers, end to end through `kw`: pre/post hooks run exactly as the tool
//! does, and the script gets back exactly the argv it was given. These are the measurements of
//! docs/explanation/args-forwarding.md, kept true by CI.
//!
//! A missing tool skips its test locally. CI installs them all and sets
//! `KIWA_REQUIRE_RUNNERS=1`, so a missing one fails there instead of passing silently.
//! Fixtures live under CARGO_TARGET_TMPDIR, outside the user profile: a stray
//! `~/package.json` with a `packageManager` field would make Corepack refuse other tools.

use std::path::{Path, PathBuf};
use std::process::Command;

const SHOW: &str = r#"console.log("ARGS=" + JSON.stringify(process.argv.slice(2)))"#;
/// Shapes that broke at least one runner without the right separator (see the research doc).
const ARGS: &[&str] = &["-x", "--watch", "a b", "c\"d", "--", "y"];

fn required() -> bool {
    std::env::var("KIWA_REQUIRE_RUNNERS").is_ok_and(|v| v == "1")
}

fn available(tool: &str) -> bool {
    let ok = which::which(tool).is_ok();
    if !ok && required() {
        panic!("{tool} is required in CI (KIWA_REQUIRE_RUNNERS=1) but not installed");
    }
    ok
}

fn fixture(name: &str, package_manager: Option<&str>) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("real-pm")
        .join(name);
    let _ = std::fs::remove_dir_all(&root);
    let p = root.join("proj");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("show.js"), SHOW).unwrap();
    let pm = package_manager
        .map(|v| format!(r#""packageManager": "{v}","#))
        .unwrap_or_default();
    std::fs::write(
        p.join("package.json"),
        format!(
            r#"{{ "name": "proj", "version": "1.0.0", {pm} "scripts": {{
                "prebuild": "node -e \"console.log('PRE-RAN')\"",
                "build": "node show.js" }} }}"#
        ),
    )
    .unwrap();
    root
}

fn kw(root: &Path, args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_kw"))
        .args(["-w", &root.to_string_lossy()])
        .args(args)
        .env("KIWA_CONFIG", root.join("config.kdl"))
        .env("KIWA_STATE", root.join("state.json"))
        .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o.status.code().unwrap_or(-1), all)
}

fn task_names(root: &Path) -> Vec<String> {
    let (_, out) = kw(root, &["info", "proj", "--json"]);
    let start = out.find('{').unwrap();
    let v: serde_json::Value = serde_json::from_str(out[start..].lines().next().unwrap()).unwrap();
    v["data"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect()
}

/// Runs `build` with the tricky arguments; checks the hook count and the argv.
fn check(root: &Path, expect_hook_runs: usize) {
    let mut args = vec!["proj", "build", "--"];
    args.extend_from_slice(ARGS);
    let (code, out) = kw(root, &args);
    assert_eq!(code, 0, "{out}");
    let want = format!("ARGS={}", serde_json::to_string(ARGS).unwrap());
    assert!(
        out.contains(&want),
        "arguments changed on the way:\nwant {want}\n{out}"
    );
    let pre = out.lines().filter(|l| l.trim() == "PRE-RAN").count();
    assert_eq!(pre, expect_hook_runs, "prebuild ran {pre} times:\n{out}");
}

#[test]
fn npm_runs_hooks_and_needs_a_separator() {
    if !available("npm") {
        return;
    }
    let root = fixture("npm", None);
    std::fs::write(root.join("proj/package-lock.json"), "{}").unwrap();
    assert_eq!(task_names(&root), ["build"], "prebuild folded into build");
    check(&root, 1);
}

#[test]
fn pnpm_9_runs_hooks_without_a_separator() {
    if !available("pnpm") {
        return;
    }
    let root = fixture("pnpm", Some("pnpm@9.0.5"));
    assert_eq!(task_names(&root), ["build"]);
    check(&root, 1);
}

#[test]
fn yarn_1_runs_hooks_and_needs_a_separator() {
    if !available("yarn") {
        return;
    }
    let root = fixture("yarn1", Some("yarn@1.22.22"));
    std::fs::write(root.join("proj/yarn.lock"), "# yarn lockfile v1\n").unwrap();
    assert_eq!(task_names(&root), ["build"]);
    check(&root, 1);
}

/// Whether `yarn` in `dir` is Yarn 2 or later, as Corepack provides it for a project whose
/// `packageManager` asks for one. A global Yarn 1 without Corepack is not: the test is then
/// skipped locally, and fails in CI where every runner is required.
fn yarn_berry_in(dir: &Path) -> bool {
    let version = which::which("yarn").ok().and_then(|yarn| {
        Command::new(yarn)
            .arg("--version")
            .current_dir(dir)
            .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    });
    let berry = version
        .as_deref()
        .and_then(|v| v.split('.').next()?.parse::<u32>().ok())
        .is_some_and(|major| major >= 2);
    if !berry && required() {
        panic!(
            "Yarn 2+ is required in CI (KIWA_REQUIRE_RUNNERS=1) but `yarn --version` gave {version:?}: enable Corepack"
        );
    }
    berry
}

#[test]
fn yarn_4_skips_hooks_and_lists_them() {
    if !available("yarn") {
        return;
    }
    let root = fixture("yarn4", Some("yarn@4.5.3"));
    let proj = root.join("proj");
    if !yarn_berry_in(&proj) {
        return;
    }
    // Yarn 2+ refuses to run scripts before an install.
    let yarn = which::which("yarn").unwrap();
    let st = Command::new(yarn)
        .arg("install")
        .current_dir(&proj)
        .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
        .env("YARN_ENABLE_IMMUTABLE_INSTALLS", "false")
        .output()
        .unwrap();
    assert!(
        st.status.success(),
        "yarn install: {}",
        String::from_utf8_lossy(&st.stderr)
    );
    assert_eq!(
        task_names(&root),
        ["prebuild", "build"],
        "hooks stay runnable"
    );
    check(&root, 0);
}

#[test]
fn bun_runs_hooks_without_a_separator() {
    if !available("bun") {
        return;
    }
    let root = fixture("bun", Some("bun@1.2.0"));
    assert_eq!(task_names(&root), ["build"]);
    check(&root, 1);
}
