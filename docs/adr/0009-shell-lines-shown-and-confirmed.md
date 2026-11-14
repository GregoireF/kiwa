# ADR 0009: A shell line runs only once it has been shown and confirmed

Status: accepted (2026-11-13). Amends the "no shell" part of [ADR 0003](0003-static-discovery.md):
discovery stays static.

## Context
kiwa ran every task as an argv, never through a shell: what runs is exactly what is listed, and
quoting cannot change it. New task sources are written as shell lines, though: README quick-start
blocks, `run:` steps of CI jobs, `Procfile` processes, `shell` tasks of `.vscode/tasks.json`,
`devcontainer.json` lifecycle commands. Most of them are plain commands; some use `&&`, pipes,
redirections or `$VAR`. Ignoring those lines would hide exactly the steps a newcomer needs.

## Decision
- A line a shell would run unchanged as an argv (no operator, expansion, glob, grouping, comment,
  escape, continuation or leading `NAME=value`, quoted text being literal) becomes that argv, as
  before. `kiwa_core::words::needs_shell` decides; it errs towards the shell.
- Any other line becomes `sh -c <line> sh`, marked `shell: true`. `sh` is named in the task, on
  every OS; on Windows the `sh.exe` of Git for Windows is found even when it is not on `PATH`.
  Extra arguments arrive as `$1`, `$2`… (`$@`), never pasted into the line.
- A `shell` task is shown as written and runs only after a confirmation: `y` in the picker, a
  `[y/N]` question in direct mode, `--yes` without a terminal (exit 2, `confirmation-required`
  otherwise). Risky task names (`deploy`, `drop`, `…:prod`, the `risky` config key) use the same
  confirmation.
- Dry runs (`-n`) show the line and need no confirmation.
- The behaviour is measured on the real shells in CI (`exec::tests`), like the runners (ADR 0006).

## Consequences
+ The steps people actually document become runnable, without kiwa ever guessing what a line does.
+ Nothing that needs a shell runs unseen; agents get `shell` and `risky` in JSON and must pass
  `--yes` deliberately.
- One more key for shell lines and risky tasks.
- On Windows, shell lines need Git for Windows (or another `sh`); kiwa says so with the install line.
