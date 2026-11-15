# Architecture

```
crates/
  kiwa-core/   library: model, detectors, discovery, resolution, config, state, setup plans
  kiwa/        the program (library kiwa_cli): CLI (clap), picker (ratatui), output, execution;
               two binaries, `kiwa` and `kw`, run the same code (ADR 0010)
```

One model (`Project`, `Task`) feeds every front end: human text, the picker and `--json`.
Front ends never re-implement discovery or resolution.

## Pipeline

1. **Walk** each root with `ignore` (honours `.gitignore`, skips hidden and build dirs,
   bounded depth). Only directories are collected.
2. **Detect** in parallel (`rayon`): each detector is a pure function
   `(dir, root) -> Option<Detection>` that reads manifest files. No process is spawned.
3. **Assemble** projects: id = path relative to the root, ecosystems in detector order,
   git branch read from `.git/HEAD`.
4. **Resolve** user input: exact → case-insensitive → unique prefix; fuzzy only suggests.
5. **Execute** an argv directly, in the project directory, with the runner resolved through
   `PATH`/`PATHEXT`. A line that needs a shell (a README step, a CI job) runs through `sh -c`
   only after it is shown and confirmed (ADR 0009). Signals are forwarded and the task's
   process tree is stopped with kw. The exit code is mirrored.

## Principles

- **Static discovery.** Listing tasks of an untrusted repository must be safe. Detectors
  parse files; they do not run `just --list` or `npm`. Trade-off: rare dynamic task
  definitions are missed. User-defined KDL detectors will cover in-house cases.
- **No shell by default.** Avoids quoting bugs and injection, and behaves the same on every
  OS; the few shell lines are always shown first.
- **Contract first.** JSON shapes and exit codes are documented before they ship.
- **Dependency budget.** Each dependency must be widely used, maintained and
  cross-platform. No YAML parser: the Taskfile scan is a small, tested line parser.
- **Fail soft on user data.** A broken manifest skips that detector; a corrupt state file
  resets history. Only an invalid config is an error, because the user can fix it.

## Adding a detector

Step by step in [CONTRIBUTING.md](../../CONTRIBUTING.md#add-a-detector).
