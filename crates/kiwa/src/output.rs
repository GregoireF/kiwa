// SPDX-License-Identifier: MIT OR Apache-2.0

//! All user-facing output. Human mode: text on stdout, diagnostics on stderr.
//! JSON mode: exactly one `kiwa/v1` envelope on stdout (see docs/reference/json-and-exit-codes.md).

use crate::exec::{Outcome, display_argv};
use kiwa_core::resolve::ResolveError;
use kiwa_core::{Project, SCHEMA, Task};
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug)]
pub struct Failure {
    pub code: i32,
    pub kind: &'static str,
    pub message: String,
    pub suggestions: Vec<String>,
}

impl Failure {
    pub fn new(code: i32, kind: &'static str, message: String) -> Self {
        Self {
            code,
            kind,
            message,
            suggestions: Vec::new(),
        }
    }
    pub fn with_suggestions(mut self, s: Vec<String>) -> Self {
        self.suggestions = s;
        self
    }
}

impl From<ResolveError> for Failure {
    fn from(e: ResolveError) -> Self {
        let code = if e.kind == "ambiguous" { 4 } else { 3 };
        Failure {
            code,
            kind: e.kind,
            message: e.message,
            suggestions: e.suggestions,
        }
    }
}

pub struct Out {
    pub json: bool,
}

fn emit(kind: &str, data: impl Serialize) {
    let v = json!({ "schema": SCHEMA, "ok": true, "kind": kind, "data": data });
    println!("{v}");
}

impl Out {
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    pub fn failure(&self, f: &Failure) {
        if self.json {
            let v = json!({
                "schema": SCHEMA, "ok": false,
                "error": { "code": f.code, "kind": f.kind, "message": f.message, "suggestions": f.suggestions }
            });
            println!("{v}");
        } else {
            eprintln!("kiwa: error: {}", f.message);
            if !f.suggestions.is_empty() {
                eprintln!("  did you mean: {}", f.suggestions.join(", "));
            }
        }
    }

    pub fn value(&self, kind: &str, v: &impl Serialize) {
        if self.json {
            emit(kind, v);
        } else {
            println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
        }
    }

    pub fn list(&self, projects: &[Project], elapsed: Duration, roots: &[PathBuf], tasks: bool) {
        let n_tasks: usize = projects.iter().map(|p| p.tasks.len()).sum();
        if self.json {
            emit(
                "project-list",
                json!({
                    "projects": projects,
                    "stats": { "projects": projects.len(), "tasks": n_tasks, "scan_ms": elapsed.as_millis(), "roots": roots }
                }),
            );
            return;
        }
        let w = projects
            .iter()
            .map(|p| p.id.chars().count())
            .max()
            .unwrap_or(7)
            .max(7);
        let stack = |p: &Project| {
            p.ecosystems
                .iter()
                .map(|e| e.label())
                .collect::<Vec<_>>()
                .join(",")
        };
        let sw = projects
            .iter()
            .map(|p| stack(p).len())
            .max()
            .unwrap_or(5)
            .max(5);
        println!(
            "{:<w$}  {:<sw$}  {:<14}  TASKS",
            "PROJECT", "STACK", "BRANCH"
        );
        for p in projects {
            println!(
                "{:<w$}  {:<sw$}  {:<14}  {}",
                p.id,
                stack(p),
                p.branch.as_deref().unwrap_or("-"),
                p.tasks.len()
            );
            if tasks {
                for t in &p.tasks {
                    println!("{:<w$}    {:<16} {}", "", t.name, display_argv(&t.command));
                }
            }
            for warning in &p.warnings {
                println!("{:<w$}    ⚠ {warning}", "");
            }
        }
        eprintln!(
            "\n{} projects · {} tasks · scanned in {} ms",
            projects.len(),
            n_tasks,
            elapsed.as_millis()
        );
    }

    pub fn info(&self, p: &Project) {
        if self.json {
            emit("project", p);
            return;
        }
        println!("{}  ({})", p.id, p.path.display());
        let stack: Vec<_> = p.ecosystems.iter().map(|e| e.label()).collect();
        println!("stack:  {}", stack.join(", "));
        if let Some(b) = &p.branch {
            println!("branch: {b}");
        }
        let w = p.tasks.iter().map(|t| t.name.len()).max().unwrap_or(4);
        println!("tasks:");
        for t in &p.tasks {
            println!("  {:<w$}  {}", t.name, display_argv(&t.command));
        }
        for warning in &p.warnings {
            println!("⚠ {warning}");
        }
    }

    pub fn path(&self, p: &Project) {
        if self.json {
            emit("path", json!({ "project": p.id, "path": p.path }));
        } else {
            println!("{}", p.path.display());
        }
    }

    pub fn dry_run(&self, p: &Project, t: &Task, extra: &[String]) {
        let argv = crate::exec::argv(t, extra);
        if self.json {
            emit(
                "dry-run",
                json!({ "project": p.id, "task": t.name, "cwd": p.path, "argv": argv }),
            );
        } else {
            println!(
                "would run in {}:\n  {}",
                p.path.display(),
                display_argv(&argv)
            );
        }
    }

    pub fn starting(&self, p: &Project, t: &Task, extra: &[String]) {
        if !self.json {
            eprintln!("▶ {} › {}   {}", p.id, t.name, crate::exec::shown(t, extra));
            let st = kiwa_core::state::State::load();
            let last = |path: &std::path::Path, task: &str| {
                st.history
                    .iter()
                    .find(|e| e.path == path && e.task == task)
                    .map(|e| e.last)
            };
            for line in crate::exec::url_notes(t, std::slice::from_ref(p), last) {
                eprintln!("  {line}");
            }
        }
    }

    pub fn finished(&self, p: &Project, t: &Task, o: &Outcome) {
        if self.json {
            emit(
                "run",
                json!({ "project": p.id, "task": t.name, "exit_code": o.code, "duration_ms": o.duration.as_millis() }),
            );
        } else {
            let mark = if o.code == 0 { "✔" } else { "✘" };
            eprintln!(
                "{mark} {} › {} · {} · {:.1}s",
                p.id,
                t.name,
                crate::exec::describe_exit(o.code),
                o.duration.as_secs_f64()
            );
        }
    }
}
