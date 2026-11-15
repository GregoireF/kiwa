// SPDX-License-Identifier: MIT OR Apache-2.0

//! Turning user (or agent) input into a project and a task.
//!
//! Safety rule: fuzzy matching is used for *suggestions only*. A name that does not
//! match exactly, case-insensitively or as a unique prefix is an error, so an agent
//! can never run the wrong task because of a typo.

use crate::fuzzy;
use crate::model::{Project, Task};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ResolveError {
    /// `not-found` | `ambiguous`
    pub kind: &'static str,
    pub message: String,
    pub suggestions: Vec<String>,
}

fn pick<'a, T>(
    items: &'a [T],
    query: &str,
    keys: impl Fn(&T) -> Vec<String>,
    what: &str,
) -> Result<&'a T, ResolveError> {
    let lower = query.to_lowercase();
    let stages: [&dyn Fn(&str) -> bool; 3] = [
        &|k: &str| k == query,
        &|k: &str| k.to_lowercase() == lower,
        &|k: &str| k.to_lowercase().starts_with(&lower),
    ];
    for stage in stages {
        let hits: Vec<&T> = items
            .iter()
            .filter(|i| keys(i).iter().any(|k| stage(k)))
            .collect();
        match hits.len() {
            0 => continue,
            1 => return Ok(hits[0]),
            _ => {
                return Err(ResolveError {
                    kind: "ambiguous",
                    message: format!("`{query}` matches several {what}s"),
                    suggestions: hits
                        .iter()
                        .map(|h| keys(h).last().cloned().unwrap_or_default())
                        .collect(),
                });
            }
        }
    }
    let hay: Vec<String> = items.iter().map(|i| keys(i).join(" ")).collect();
    Err(ResolveError {
        kind: "not-found",
        message: format!("no {what} named `{query}`"),
        suggestions: fuzzy::rank(query, &hay)
            .into_iter()
            .take(3)
            .map(|i| keys(&items[i]).last().cloned().unwrap_or_default())
            .collect(),
    })
}

pub fn project<'a>(projects: &'a [Project], query: &str) -> Result<&'a Project, ResolveError> {
    pick(
        projects,
        query,
        |p| vec![p.name.clone(), p.id.clone()],
        "project",
    )
}

/// Accepts `name` or `runner:name` (e.g. `make:build` when npm also has `build`).
pub fn task<'a>(project: &'a Project, query: &str) -> Result<&'a Task, ResolveError> {
    pick(
        &project.tasks,
        query,
        |t| vec![t.name.clone(), t.qualified()],
        "task",
    )
    .map_err(|mut e| {
        if e.kind == "not-found" && e.suggestions.is_empty() {
            e.suggestions = project.tasks.iter().map(|t| t.name.clone()).collect();
        }
        e.message = format!("{} in project `{}`", e.message, project.id);
        e
    })
}
