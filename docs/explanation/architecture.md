# Architecture

```
crates/
  kiwa-core/   library: model, detectors, discovery, resolution, config, state
  kiwa/        the `kw` binary: CLI (clap), TUI (ratatui), output, process execution
```

One model (`Project`, `Task`) feeds every front end: human text, the TUI, `--json` and,
next, the MCP server. Front ends never re-implement discovery or resolution.

## Pipeline

1. **Walk** each root with `ignore` (honours `.gitignore`, skips hidden and build dirs,
   bounded depth). Only directories are collected.
2. **Detect** in parallel (`rayon`): each detector is a pure function
   `(dir, root) -> Option<Detection>` that reads manifest files. No process is spawned.
3. **Assemble** projects: id = path relative to the root, ecosystems in detector order,
   git branch read from `.git/HEAD`.
4. **Resolve** user input: exact → case-insensitive → unique prefix; fuzzy only suggests.
5. **Execute** an argv directly (no shell), in the project directory, with the runner
   resolved through `PATH`/`PATHEXT`. The exit code is mirrored.

## Principles

- **Static discovery.** Listing tasks of an untrusted repository must be safe. Detectors
  parse files; they do not run `just --list` or `npm`. Trade-off: rare dynamic task
  definitions are missed. User-defined KDL detectors will cover in-house cases.
- **No shell.** Avoids quoting bugs and injection, and behaves the same on every OS.
- **Contract first.** JSON shapes and exit codes are documented before they ship.
- **Dependency budget.** Each dependency must be widely used, maintained and
  cross-platform. No YAML parser: the Taskfile scan is a small, tested line parser.
- **Fail soft on user data.** A broken manifest skips that detector; a corrupt state file
  resets history. Only an invalid config is an error, because the user can fix it.

## Adding a detector

1. Add a variant to `Ecosystem` in `model.rs`.
2. Write `fn name(dir: &Path, root: &Path) -> Option<Detection>` in `detect.rs` and add it
   to `DETECTORS`.
3. Add a fixture to `examples/demo` and a unit test for any parsing logic.
