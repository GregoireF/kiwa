// SPDX-License-Identifier: MIT OR Apache-2.0

//! Library-level tests: detector edge cases, resolution, ranking, git.

use kiwa_core::detect::{
    detect_dir, just_params, parse_just_recipes, parse_make_targets, parse_rake_tasks,
    parse_taskfile_tasks, script_params, taskfile_requires,
};
use kiwa_core::model::{Ecosystem, Param, ParamForm, Project, Task};

fn usage(ps: &[Param]) -> String {
    ps.iter().map(Param::usage).collect::<Vec<_>>().join(" ")
}

#[test]
fn script_folders_add_tasks_to_projects_only() {
    use kiwa_core::detect::script_argv;
    let v = |rel: &str, head: &str| script_argv(rel, head).map(|a| a.join(" "));
    assert_eq!(
        v("scripts/x", "#!/usr/bin/env python3 -u\n"),
        Some("python3 -u scripts/x".into())
    );
    assert_eq!(
        v("bin/rails", "#!/usr/bin/env ruby\n"),
        Some("ruby bin/rails".into())
    );
    assert_eq!(v("bin/x", "#!/bin/bash -e\n"), Some("bash -e bin/x".into()));
    assert_eq!(v("tools/a.sh", "echo"), Some("sh tools/a.sh".into()));
    assert_eq!(
        v("tools/a.ps1", ""),
        Some("pwsh -NoProfile -File tools/a.ps1".into())
    );
    assert_eq!(v("tools/a.mjs", ""), Some("node tools/a.mjs".into()));
    assert_eq!(v("bin/app", "\x7fELF\x02"), None, "a compiled program");
    assert_eq!(v("bin/app.dll", "MZ"), None);
    assert_eq!(v("bin/x", "#!\n"), None, "an empty shebang");

    let d = tmp();
    fs::create_dir_all(d.path().join("scripts")).unwrap();
    fs::create_dir_all(d.path().join("bin")).unwrap();
    fs::write(
        d.path().join("scripts/deploy.sh"),
        "#!/usr/bin/env bash\n# Ship to production\nset -e\n",
    )
    .unwrap();
    fs::write(d.path().join("scripts/.hidden.sh"), "").unwrap();
    fs::write(d.path().join("scripts/notes.txt"), "x").unwrap();
    fs::write(d.path().join("bin/tool.dll"), "MZ").unwrap();
    // Without a manifest or a repository, script folders are not a project (`~/bin`).
    assert!(detect_dir(d.path(), d.path()).is_empty());
    fs::write(d.path().join("Makefile"), "all:\n\techo\n").unwrap();
    let det = detect_dir(d.path(), d.path());
    let s = det
        .iter()
        .find(|x| x.ecosystem == Ecosystem::Scripts)
        .unwrap();
    assert_eq!(s.from, "scripts/, bin/");
    assert_eq!(s.tasks.len(), 1);
    let t = &s.tasks[0];
    assert_eq!((t.name.as_str(), t.runner.as_str()), ("deploy", "scripts"));
    assert_eq!(t.command, ["bash", "scripts/deploy.sh"]);
    assert_eq!(
        t.description.as_deref(),
        Some("scripts/deploy.sh · Ship to production")
    );
    // A bare git repository with scripts lists them, and is still marked as git.
    let r = tmp();
    fs::create_dir_all(r.path().join(".git")).unwrap();
    fs::create_dir_all(r.path().join("tools")).unwrap();
    fs::write(r.path().join("tools/sync.py"), "print(1)").unwrap();
    let det = detect_dir(r.path(), r.path());
    let ecos: Vec<_> = det.iter().map(|x| x.ecosystem).collect();
    assert_eq!(ecos, [Ecosystem::Scripts, Ecosystem::Git]);
}

#[test]
fn nx_targets_and_turbo_tasks() {
    let d = tmp();
    let app = d.path().join("apps/web");
    fs::create_dir_all(&app).unwrap();
    fs::write(
        d.path().join("nx.json"),
        r#"{ "targetDefaults": { "lint": {}, "build": {}, "@nx/vite:test": {} } }"#,
    )
    .unwrap();
    fs::write(
        app.join("project.json"),
        r#"{ // Nx project
  "name": "web",
  "targets": {
    "build": { "executor": "@nx/vite:build" },
    "serve": { "options": { "command": "vite" } },
  }
}"#,
    )
    .unwrap();
    let det = detect_dir(&app, d.path());
    let nx = det.iter().find(|x| x.ecosystem == Ecosystem::Nx).unwrap();
    let names: Vec<_> = nx.tasks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        ["build", "serve", "lint"],
        "defaults added, executor keys skipped"
    );
    assert_eq!(nx.tasks[1].command, ["npx", "nx", "run", "web:serve"]);
    assert_eq!(nx.tasks[0].description.as_deref(), Some("@nx/vite:build"));
    fs::write(app.join("project.json"), "{ nope").unwrap();
    let det = detect_dir(&app, d.path());
    assert!(
        det.iter()
            .any(|x| x.ecosystem == Ecosystem::Nx && !x.warnings.is_empty())
    );

    fs::write(
        d.path().join("turbo.json"),
        r#"{ "$schema": "x", "tasks": { "build": {}, "web#deploy": {}, "//#check": {} } }"#,
    )
    .unwrap();
    let det = detect_dir(d.path(), d.path());
    let t = det
        .iter()
        .find(|x| x.ecosystem == Ecosystem::Turbo)
        .unwrap();
    let cmd = |n: &str| {
        t.tasks
            .iter()
            .find(|x| x.name == n)
            .unwrap_or_else(|| panic!("{n}"))
            .command
            .join(" ")
    };
    assert_eq!(cmd("turbo:build"), "npx turbo run build");
    assert_eq!(cmd("turbo:web#deploy"), "npx turbo run deploy --filter web");
    assert_eq!(cmd("turbo:check"), "npx turbo run check");
    fs::write(
        d.path().join("turbo.json"),
        r#"{ "pipeline": { "test": {} } }"#,
    )
    .unwrap();
    let det = detect_dir(d.path(), d.path());
    assert!(
        det.iter()
            .any(|x| x.tasks.iter().any(|t| t.name == "turbo:test")),
        "turbo 1"
    );
}

#[test]
fn compose_services_with_profiles() {
    let s = kiwa_core::detect::parse_compose_services(
        "name: shop\nservices:\n  # the API\n  api:\n    image: x\n    ports: [\"8080:80\"]\n  \"db\":\n    image: postgres\n  debug:\n    profiles: [tools, dev]\n  mail:\n    profiles:\n      - \"mail\"\n    image: y\nvolumes:\n  data: {}\n",
    );
    let names: Vec<_> = s.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["api", "db", "debug", "mail"]);
    assert_eq!(s[2].1, ["tools", "dev"]);
    assert_eq!(s[3].1, ["mail"]);

    let d = tmp();
    fs::write(
        d.path().join("compose.yaml"),
        "services:\n  api:\n    image: x\n  debug:\n    profiles: [tools]\n",
    )
    .unwrap();
    let tasks: Vec<Task> = detect_dir(d.path(), d.path())
        .into_iter()
        .flat_map(|x| x.tasks)
        .collect();
    let cmd = |n: &str| {
        tasks
            .iter()
            .find(|t| t.name == n)
            .unwrap()
            .command
            .join(" ")
    };
    assert_eq!(cmd("compose:up"), "docker compose up");
    assert_eq!(cmd("api:up"), "docker compose up api");
    assert_eq!(cmd("api:logs"), "docker compose logs -f api");
    assert_eq!(cmd("api:sh"), "docker compose exec api sh");
    assert_eq!(cmd("debug:up"), "docker compose --profile tools up debug");
    assert_eq!(tasks.len(), 4 + 2 * 5);
}

#[test]
fn devcontainer_lifecycle_commands() {
    let d = tmp();
    fs::create_dir_all(d.path().join(".devcontainer/python")).unwrap();
    fs::write(
        d.path().join(".devcontainer/devcontainer.json"),
        r#"{
  // JSONC: comments and trailing commas
  "name": "app", /* block */
  "image": "mcr.microsoft.com/devcontainers/base", "url": "http://x//y",
  "initializeCommand": "echo host",
  "postCreateCommand": "npm ci && npm run build",
  "postStartCommand": ["npm", "run", "dev"],
  "postAttachCommand": { "server": "npm start", "db": ["make", "db"], },
}"#,
    )
    .unwrap();
    fs::write(
        d.path().join(".devcontainer/python/devcontainer.json"),
        r#"{ "onCreateCommand": "uv sync" }"#,
    )
    .unwrap();
    let det: Vec<_> = detect_dir(d.path(), d.path());
    let dc = det
        .iter()
        .find(|x| x.ecosystem == Ecosystem::Devcontainer)
        .unwrap();
    assert_eq!(dc.from, ".devcontainer/devcontainer.json");
    assert!(dc.warnings.is_empty(), "{:?}", dc.warnings);
    let t = |n: &str| {
        dc.tasks
            .iter()
            .find(|t| t.name == n)
            .unwrap_or_else(|| panic!("{n}"))
    };
    assert_eq!(
        t("devcontainer:up").command.join(" "),
        "devcontainer up --workspace-folder ."
    );
    assert_eq!(
        t("devcontainer:initialize").command,
        ["echo", "host"],
        "on the host"
    );
    let post = t("devcontainer:postCreate");
    assert!(post.shell);
    assert_eq!(post.line(), Some("npm ci && npm run build"));
    assert_eq!(
        post.command[..4],
        ["devcontainer", "exec", "--workspace-folder", "."]
    );
    assert_eq!(
        t("devcontainer:postStart").command.join(" "),
        "devcontainer exec --workspace-folder . npm run dev"
    );
    assert!(!t("devcontainer:postAttach:server").shell);
    assert_eq!(
        t("devcontainer:postAttach:db").command.last().unwrap(),
        "db"
    );
    assert_eq!(
        t("devcontainer:python:onCreate").command.join(" "),
        "devcontainer exec --workspace-folder . --config .devcontainer/python/devcontainer.json uv sync"
    );
    assert_eq!(
        kiwa_core::detect::jsonc(r#"{"a": "x // y, ]", "b": [1, 2,], /* c */}"#),
        r#"{"a": "x // y, ]", "b": [1, 2] }"#
    );
}

#[test]
fn just_parameters_without_a_default_are_required() {
    let p = just_params(
        "set shell := [\"bash\"]\nx := \"1\"\n# deploy it\ndeploy env target=\"a: b\" +files *rest: build\n  echo\n@greet $name = 'you':\n  echo\nlint:\n  echo\nexpr arch=(os() + \"x\") count:\n  echo\n",
    );
    assert_eq!(usage(&p["deploy"]), "<env> <files>…");
    assert!(!p.contains_key("greet"), "a default with spaces around =");
    assert!(!p.contains_key("lint"));
    assert_eq!(usage(&p["expr"]), "<count>", "parenthesised default");
}

#[test]
fn taskfile_required_variables() {
    let p = taskfile_requires(
        "version: '3'\ntasks:\n  deploy:\n    desc: Ship\n    requires:\n      vars: [ENV, \"REGION\"]\n    cmds:\n      - echo\n  migrate:\n    requires:\n      vars:\n        - DB\n        - name: MODE\n          enum: [up, down]\n    cmds: [echo]\n  test:\n    cmds: [echo]\nvars:\n  X: 1\n",
    );
    assert_eq!(usage(&p["deploy"]), "ENV=… REGION=…");
    assert_eq!(usage(&p["migrate"]), "DB=… MODE=…");
    assert!(!p.contains_key("test"));
    assert!(p["deploy"].iter().all(|x| x.form == ParamForm::Variable));
}

#[test]
fn scripts_that_are_only_a_prefix() {
    let u = |c: &str| usage(&script_params(c));
    assert_eq!(u("pnpm -r"), "<command>…");
    assert_eq!(u("pnpm --filter web"), "<command>…");
    assert_eq!(u("pnpm --filter"), "<filter> <command>…");
    assert_eq!(u("turbo run"), "<task>…");
    assert_eq!(u("turbo run build --filter"), "<filter>");
    assert_eq!(u("npx"), "<command>…");
    assert_eq!(u("npm run"), "<script>…");
    assert_eq!(u("yarn workspace"), "<workspace> <command>…");
    for complete in [
        "vite",
        "turbo run build",
        "pnpm -r build",
        "npx prettier --write .",
        "pnpm -r && echo",
        "yarn",
        "",
    ] {
        assert_eq!(u(complete), "", "{complete}");
    }
}

#[test]
fn detectors_attach_params_to_tasks() {
    let d = tmp();
    fs::write(
        d.path().join("justfile"),
        "deploy env:\n  echo\nok:\n  echo\n",
    )
    .unwrap();
    fs::write(
        d.path().join("package.json"),
        r#"{"scripts":{"ws":"pnpm --filter","dev":"vite"}}"#,
    )
    .unwrap();
    fs::write(
        d.path().join("Taskfile.yml"),
        "tasks:\n  up:\n    requires:\n      vars: [ENV]\n",
    )
    .unwrap();
    let tasks: Vec<Task> = detect_dir(d.path(), d.path())
        .into_iter()
        .flat_map(|x| x.tasks)
        .collect();
    let of = |n: &str, r: &str| {
        tasks
            .iter()
            .find(|t| t.name == n && t.runner == r)
            .map(Task::params_usage)
            .unwrap()
    };
    assert_eq!(of("deploy", "just"), "<env>");
    assert_eq!(of("ok", "just"), "");
    assert_eq!(of("ws", "npm"), "<filter> <command>…");
    assert_eq!(of("dev", "npm"), "");
    assert_eq!(of("up", "task"), "ENV=…");
}
use kiwa_core::state::State;
use kiwa_core::{DiscoverOptions, discover, fuzzy, git, resolve};
use std::fs;
use std::path::{Path, PathBuf};

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

fn tasks(dir: &Path) -> Vec<String> {
    detect_dir(dir, dir)
        .into_iter()
        .flat_map(|d| d.tasks.into_iter().map(|t| t.name))
        .collect()
}

fn ecos(dir: &Path) -> Vec<Ecosystem> {
    detect_dir(dir, dir)
        .into_iter()
        .map(|d| d.ecosystem)
        .collect()
}

// ------------------------------------------------------------------ make

#[test]
fn make_ignores_variables_patterns_and_special_targets() {
    let t = parse_make_targets(
        "A = 1\nB ?= 2\nC += 3\nD := 4\n%.o: %.c\n.DEFAULT_GOAL := all\n$(OUT): x\nall:\n",
    );
    assert_eq!(t, [("all".to_string(), None)]);
}

#[test]
fn make_double_colon_and_multiple_targets() {
    let t = parse_make_targets("clean::\n\trm x\nfmt lint: ## Style\n");
    let names: Vec<_> = t.iter().map(|x| x.0.as_str()).collect();
    assert_eq!(names, ["clean", "fmt", "lint"]);
    assert_eq!(t[2].1.as_deref(), Some("Style"));
}

#[test]
fn make_deduplicates() {
    assert_eq!(parse_make_targets("a:\na: b\n").len(), 1);
}

#[test]
fn make_file_variants() {
    for f in ["Makefile", "makefile", "GNUmakefile"] {
        let d = tmp();
        write(d.path(), f, "x:\n");
        assert_eq!(tasks(d.path()), ["x"], "{f}");
    }
}

// ------------------------------------------------------------------ just

#[test]
fn just_parameters_attributes_and_aliases() {
    let r = parse_just_recipes(
        "alias b := build\n[private]\nhelper:\n  x\n# Build it\n[group('ci')]\nbuild target='debug':\n  x\nexport FOO := '1'\nimport 'other.just'\n",
    );
    let names: Vec<_> = r.iter().map(|x| x.0.as_str()).collect();
    assert!(names.contains(&"build"));
    assert!(!names.contains(&"b"));
    let build = r.iter().find(|x| x.0 == "build").unwrap();
    assert_eq!(
        build.1.as_deref(),
        Some("Build it"),
        "doc comment survives attributes"
    );
}

#[test]
fn just_shebang_comment_is_not_a_doc() {
    let r = parse_just_recipes("#!/usr/bin/env just --justfile\nrun:\n  x\n");
    assert_eq!(r, [("run".to_string(), None)]);
}

#[test]
fn just_file_variants() {
    for f in ["justfile", "Justfile", ".justfile"] {
        let d = tmp();
        write(d.path(), f, "go:\n  x\n");
        assert_eq!(tasks(d.path()), ["go"], "{f}");
    }
}

// ------------------------------------------------------------------ Taskfile

#[test]
fn taskfile_quoted_names_and_four_space_indent() {
    let t = parse_taskfile_tasks(
        "version: '3'\ntasks:\n    \"build:web\":\n        desc: \"Web\"\n    test: { cmds: [x] }\n",
    );
    assert_eq!(t[0], ("build:web".to_string(), Some("Web".to_string())));
    assert_eq!(t[1].0, "test");
}

#[test]
fn taskfile_ignores_other_sections_and_comments() {
    let t = parse_taskfile_tasks(
        "vars:\n  a: 1\n# tasks:\ntasks:\n  # note\n  one:\n    cmds: [x]\nincludes:\n  two: ./x\n",
    );
    assert_eq!(t, [("one".to_string(), None)]);
}

// ------------------------------------------------------------------ rake

#[test]
fn rake_task_forms() {
    let t =
        parse_rake_tasks("task :a\ntask b: :a\ntask \"c\" do\nend\ntask(:d)\n  task :e do; end\n");
    assert_eq!(t, ["a", "b", "c", "d", "e"]);
}

// ------------------------------------------------------------------ node

#[test]
fn node_non_string_scripts_are_ignored() {
    let d = tmp();
    write(
        d.path(),
        "package.json",
        r#"{"scripts":{"a":"x","b":1,"c":null}}"#,
    );
    assert_eq!(tasks(d.path()), ["a"]);
}

#[test]
fn node_preserves_author_order() {
    let d = tmp();
    write(
        d.path(),
        "package.json",
        r#"{"scripts":{"zeta":"x","alpha":"y","mid":"z"}}"#,
    );
    assert_eq!(tasks(d.path()), ["zeta", "alpha", "mid"]);
}

#[test]
fn node_hook_without_main_script_stays() {
    let d = tmp();
    write(
        d.path(),
        "package.json",
        r#"{"scripts":{"postinstall":"x","prepare":"y"}}"#,
    );
    assert_eq!(tasks(d.path()), ["postinstall", "prepare"]);
}

#[test]
fn node_array_root_is_a_warning() {
    let d = tmp();
    write(d.path(), "package.json", "[]");
    let det = detect_dir(d.path(), d.path());
    assert_eq!(det[0].warnings.len(), 1);
}

#[test]
fn node_description_is_the_command() {
    let d = tmp();
    write(
        d.path(),
        "package.json",
        r#"{"scripts":{"dev":"vite --open"}}"#,
    );
    let t = &detect_dir(d.path(), d.path())[0].tasks[0];
    assert_eq!(t.description.as_deref(), Some("vite --open"));
}

#[test]
fn yarn_classic_lockfile_is_not_berry() {
    let d = tmp();
    write(
        d.path(),
        "package.json",
        r#"{"scripts":{"prebuild":"a","build":"b"}}"#,
    );
    write(
        d.path(),
        "yarn.lock",
        "# THIS IS AN AUTOGENERATED FILE.\n# yarn lockfile v1\n\n",
    );
    assert_eq!(tasks(d.path()), ["build"], "Yarn 1 runs hooks, fold them");
}

// ------------------------------------------------------------------ deno / rust / go

#[test]
fn deno_jsonc_with_comments_still_detected() {
    let d = tmp();
    write(d.path(), "deno.jsonc", "{ // comment\n \"tasks\": {} }");
    assert_eq!(ecos(d.path()), [Ecosystem::Deno]);
}

#[test]
fn deno_task_object_form_with_description() {
    let d = tmp();
    write(
        d.path(),
        "deno.json",
        r#"{"tasks":{"dev":{"command":"deno run x","description":"Dev server"}}}"#,
    );
    let t = &detect_dir(d.path(), d.path())[0].tasks[0];
    assert_eq!(t.description.as_deref(), Some("Dev server"));
}

#[test]
fn rust_library_has_no_run_task() {
    let d = tmp();
    write(d.path(), "Cargo.toml", "[package]\nname=\"lib\"\n");
    assert!(!tasks(d.path()).contains(&"run".to_string()));
}

#[test]
fn rust_explicit_bin_has_run() {
    let d = tmp();
    write(
        d.path(),
        "Cargo.toml",
        "[package]\nname=\"x\"\n[[bin]]\nname=\"x\"\npath=\"x.rs\"\n",
    );
    assert!(tasks(d.path()).contains(&"run".to_string()));
}

#[test]
fn rust_invalid_toml_is_not_a_crash() {
    let d = tmp();
    write(d.path(), "Cargo.toml", "[package\n");
    assert!(ecos(d.path()).is_empty());
}

#[test]
fn go_library_has_no_run_task() {
    let d = tmp();
    write(d.path(), "go.mod", "module x\n");
    assert_eq!(tasks(d.path()), ["build", "test", "vet"]);
}

// ------------------------------------------------------------------ python

#[test]
fn python_hatch_scripts() {
    let d = tmp();
    write(
        d.path(),
        "pyproject.toml",
        "[tool.hatch.envs.default.scripts]\ncov = \"pytest --cov\"\n",
    );
    let t = &detect_dir(d.path(), d.path())[0].tasks[0];
    assert_eq!(t.command, ["hatch", "run", "cov"]);
}

#[test]
fn python_user_test_script_wins_over_implicit_pytest() {
    let d = tmp();
    write(
        d.path(),
        "pyproject.toml",
        "[tool.poe.tasks]\ntest = \"pytest -q\"\n",
    );
    fs::create_dir_all(d.path().join("tests")).unwrap();
    let names = tasks(d.path());
    assert_eq!(names.iter().filter(|n| *n == "test").count(), 1);
}

#[test]
fn python_plain_requirements_has_no_runner_prefix() {
    let d = tmp();
    write(d.path(), "requirements.txt", "x\n");
    fs::create_dir_all(d.path().join("tests")).unwrap();
    let t = &detect_dir(d.path(), d.path())[0].tasks[0];
    assert_eq!(t.command, ["pytest"]);
}

// ------------------------------------------------------------------ mise / dotnet / ruby / compose

#[test]
fn mise_file_tasks() {
    let d = tmp();
    write(d.path(), "mise-tasks/deploy.sh", "#!/bin/sh\n");
    write(d.path(), ".mise/tasks/lint", "#!/bin/sh\n");
    let mut names = tasks(d.path());
    names.sort();
    assert_eq!(names, ["deploy", "lint"]);
}

#[test]
fn dotnet_solution_only_has_no_run() {
    let d = tmp();
    write(d.path(), "All.sln", "");
    assert_eq!(tasks(d.path()), ["build", "test", "restore"]);
}

#[test]
fn ruby_rakefile_without_gemfile_uses_rake_directly() {
    let d = tmp();
    write(d.path(), "Rakefile", "task :spec\n");
    let t = &detect_dir(d.path(), d.path())[0].tasks[0];
    assert_eq!(t.command, ["rake", "spec"]);
}

#[test]
fn compose_file_variants() {
    for f in [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yml",
        "docker-compose.yaml",
    ] {
        let d = tmp();
        write(d.path(), f, "services: {}\n");
        assert_eq!(ecos(d.path()), [Ecosystem::Compose], "{f}");
    }
}

#[test]
fn polyglot_folder_merges_in_detector_order() {
    let d = tmp();
    write(d.path(), "package.json", r#"{"scripts":{"a":"x"}}"#);
    write(d.path(), "Makefile", "b:\n");
    write(d.path(), "compose.yaml", "");
    assert_eq!(
        ecos(d.path()),
        [Ecosystem::Node, Ecosystem::Make, Ecosystem::Compose]
    );
}

#[test]
fn empty_folder_is_not_a_project() {
    let d = tmp();
    assert!(detect_dir(d.path(), d.path()).is_empty());
}

// ------------------------------------------------------------------ discovery

#[test]
fn root_itself_can_be_a_project() {
    let d = tmp();
    write(d.path(), "go.mod", "module root\n");
    let p = discover(&DiscoverOptions {
        roots: vec![d.path().into()],
        ..Default::default()
    });
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].id, d.path().file_name().unwrap().to_string_lossy());
}

#[test]
fn overlapping_roots_are_deduplicated() {
    let d = tmp();
    write(d.path(), "a/go.mod", "module a\n");
    let roots = vec![d.path().into(), d.path().join("a")];
    let p = discover(&DiscoverOptions {
        roots,
        ..Default::default()
    });
    assert_eq!(p.len(), 1);
}

#[test]
fn custom_ignore_names() {
    let d = tmp();
    write(d.path(), "archive/old/go.mod", "module a\n");
    write(d.path(), "live/go.mod", "module b\n");
    let p = discover(&DiscoverOptions {
        roots: vec![d.path().into()],
        max_depth: 4,
        ignore: vec!["archive".into()],
    });
    assert_eq!(
        p.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["live"]
    );
}

#[test]
fn missing_root_is_empty_not_an_error() {
    let p = discover(&DiscoverOptions {
        roots: vec![PathBuf::from("/definitely/not/here")],
        ..Default::default()
    });
    assert!(p.is_empty());
}

// ------------------------------------------------------------------ git

#[test]
fn git_branch_cases() {
    let d = tmp();
    assert_eq!(git::branch(d.path()), None, "not a repo");
    write(d.path(), ".git/HEAD", "ref: refs/heads/feat/a-b\n");
    assert_eq!(git::branch(d.path()).as_deref(), Some("feat/a-b"));
    let w = tmp();
    write(w.path(), ".git", "gitdir: ../nowhere\n");
    assert_eq!(git::branch(w.path()), None, "broken worktree link");
}

// ------------------------------------------------------------------ resolution

fn project(id: &str, tasks: &[(&str, &str)]) -> Project {
    Project {
        id: id.into(),
        name: id.rsplit('/').next().unwrap().into(),
        path: PathBuf::from(id),
        ecosystems: vec![],
        branch: None,
        default_branch: None,
        tasks: tasks
            .iter()
            .map(|(n, r)| Task::new(*n, r, &[r, n]))
            .collect(),
        warnings: vec![],
        hints: vec![],
        detected_from: vec![],
        member_of: None,
    }
}

#[test]
fn resolve_by_name_or_id() {
    let ps = [project("apps/web", &[]), project("api", &[])];
    assert_eq!(resolve::project(&ps, "apps/web").unwrap().id, "apps/web");
    assert_eq!(resolve::project(&ps, "web").unwrap().id, "apps/web");
    assert_eq!(resolve::project(&ps, "WEB").unwrap().id, "apps/web");
}

#[test]
fn resolve_same_name_in_two_places_is_ambiguous_with_ids() {
    let ps = [project("a/web", &[]), project("b/web", &[])];
    let e = resolve::project(&ps, "web").unwrap_err();
    assert_eq!(e.kind, "ambiguous");
    assert_eq!(e.suggestions, ["a/web", "b/web"]);
}

#[test]
fn resolve_typo_gives_fuzzy_suggestions_only() {
    let ps = [project("storefront", &[]), project("admin", &[])];
    let e = resolve::project(&ps, "strfront").unwrap_err();
    assert_eq!(e.kind, "not-found");
    assert_eq!(e.suggestions[0], "storefront");
}

#[test]
fn resolve_task_lists_all_when_nothing_is_close() {
    let p = project("a", &[("dev", "npm"), ("lint", "npm")]);
    let e = resolve::task(&p, "zzzzzz").unwrap_err();
    assert_eq!(e.suggestions, ["dev", "lint"]);
    assert!(e.message.contains("project `a`"));
}

#[test]
fn resolve_qualified_task() {
    let p = project("a", &[("build", "npm"), ("build", "make")]);
    assert_eq!(resolve::task(&p, "make:build").unwrap().runner, "make");
    assert_eq!(resolve::task(&p, "build").unwrap_err().kind, "ambiguous");
}

// ------------------------------------------------------------------ ranking

#[test]
fn frecency_prefers_frequent_and_recent() {
    let mut s = State::default();
    let a = Path::new("/a");
    let b = Path::new("/b");
    assert_eq!(s.score(a, "t"), 0);
    s.record(a, "t", Some(0));
    s.record(a, "t", Some(0));
    s.record(b, "t", Some(1));
    assert!(s.score(a, "t") > s.score(b, "t"));
    assert_eq!(s.history.len(), 2);
    assert_eq!(s.last().unwrap().exit_code, Some(1));
    s.clear();
    assert!(s.last().is_none());
}

#[test]
fn history_is_bounded() {
    let mut s = State::default();
    for i in 0..600 {
        s.record(Path::new(&format!("/p{i}")), "t", None);
    }
    assert_eq!(s.history.len(), 500);
}

#[test]
fn fuzzy_empty_query_keeps_order_and_no_match_is_empty() {
    assert_eq!(fuzzy::rank("", &["b", "a"]), [0, 1]);
    assert!(fuzzy::rank("zzz", &["abc"]).is_empty());
    assert_eq!(
        fuzzy::rank("STORE", &["admin", "storefront"]),
        [1],
        "case-insensitive"
    );
}
