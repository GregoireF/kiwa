---
name: kiwa
description: Discover projects and run their tasks (build, test, dev, lint...) across any language with the `kw` CLI. Use when you need to know what projects exist in a workspace, which commands a project supports, or to run a project task without guessing the package manager or task runner.
---

# kiwa (`kw`)

`kw` finds projects under a directory and lists the tasks each one exposes (npm/pnpm/yarn/bun
scripts, cargo, go, uv/poetry, make, just, Taskfile, mise, gradle, maven, dotnet, composer,
rake, docker compose). Always use `--json`.

## Discover

```sh
kw list --json            # all projects under the current directory
kw -w <dir> list --json   # under a specific root
kw info <project> --json  # one project and its tasks
```

Each task has `name`, `runner` and `command` (the exact argv that will run). A task with
`params` cannot run without input: ask the user for each value, then pass them after `--`
(`positional` and `variadic` as words, `variable` as `NAME=value`).

## Run

1. Dry-run first: `kw run <project> <task> -n --json` and check `argv` and `cwd`.
2. Run it: `kw run <project> <task> --json [-- extra args]`.
   Task output goes to stderr; the final line on stdout is
   `{"ok":true,"kind":"run","data":{"exit_code":0,...}}`.

## Errors

Read `error.kind` and `error.suggestions`. Never retry with a guessed name: pick one of
the suggestions or ask the user.

| exit | meaning |
|---|---|
| 2 | `missing-input`: the task has `params` and none were given → ask the user |
| 2 | `confirmation-required`: the task is `shell` or `risky` → show the user the command, and pass `--yes` only after they agree |
| 3 | not found → use `suggestions` |
| 4 | ambiguous → use a suggestion such as `make:build` |
| 127 | runner not installed → tell the user |
| other | the task's own exit code |

## Rules

- Do not run tasks named like `deploy`, `publish`, `release`, `drop`, `prune`, `destroy`
  or `apply` without explicit confirmation from the user.
- Prefer `kw` over reading manifests by hand: it already resolves the right package manager.
