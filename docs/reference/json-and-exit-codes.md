# CLI contract (`kiwa/v1`)

This is the machine-facing interface used by scripts, CI and AI agents. Until 1.0 it may
change, but every change is listed in the [changelog](../../CHANGELOG.md). From 1.0 on,
breaking changes require a new schema version (`kiwa/v2`). Fields may be **added** within
`kiwa/v1`; consumers must ignore unknown fields.

## Streams

- `--json` (or `KIWA_JSON=1`): **exactly one** JSON document on stdout, then exit.
- Diagnostics, progress and task output go to **stderr**. With `kw run --json`, the task's
  own stdout is redirected to stderr so stdout stays parseable.
- Without `--json`, stdout carries human-readable output; there are no colour codes
  outside the interactive picker.
- kiwa never opens the interactive picker when stdin or stdout is not a terminal.

## Envelope

Success:

```json
{ "schema": "kiwa/v1", "ok": true, "kind": "<kind>", "data": { } }
```

Failure:

```json
{ "schema": "kiwa/v1", "ok": false,
  "error": { "code": 3, "kind": "not-found", "message": "…", "suggestions": ["…"] } }
```

| `kind` | Command | `data` |
|---|---|---|
| `project-list` | `list`, `kw` without a terminal | `{ projects: Project[], stats: { projects, tasks, scan_ms, roots } }` |
| `project` | `info` | `Project` |
| `path` | `path` | `{ project, path }` |
| `dry-run` | `run -n`, `last -n` | `{ project, task, cwd, argv }` |
| `run` | `run`, `last`, direct mode | `{ project, task, exit_code, duration_ms }` |
| `init` | `init --use` | `{ config, roots, accent }` |
| `init-suggestions` | `init` without a terminal | `Suggestion[]`: `{ path, reason, projects, tasks }` |
| `aliases`, `alias-add`, `alias-remove` | `alias …` | as named |
| `config`, `config-path`, `config-set` | `config …` | as named |
| `config-get` | `config get <key>` | `{ key, value }` |
| `history` | `history` | `Entry[]`: `{ path, task, count, last, exit_code }` |
| `reset` | `reset --yes` | `{ removed: string[] }` |
| `about` | `about` | `{ id, name, description?, license?, repository?, branch?, stack[], start: { role, task, runner }[], setup: string[], requires: { tool, version, from }[] }`; `role` is `install`, `dev`, `test`, `build` or `lint`; `setup` lists what is known to be missing |
| `setup-plan` | `setup -n` | `{ project, steps: Step[], suggested: Task[] }`; `Step`: `{ what, why, done, kind: "run" \| "copy", task?, cwd?, from?, to? }` |
| `setup` | `setup` | `{ project, steps: { what, exit_code }[] }`, the steps that ran, in order; it stops at the first non-zero `exit_code`, which is also the exit code |
| `doctor` | `doctor` | `{ kiwa: { version, os, arch }, terminal: { stdin_tty, stdout_tty, program, term, size: [cols, rows]?, colours: { enabled, depth, from }, mouse: { enabled, reason } }, config: Files, state: Files, runners: { name, path, hint? }[] }`; `Files`: `{ path, exists, valid, error, warnings, runs? }`; `depth`: `truecolor`, `256`, `none` or `unknown` |
| `doctor-probe` | `doctor --probe` | `{ program, seen: string[] }`, `seen` among `left-click`, `right-click`, `middle-click`, `motion`, `drag`, `wheel-up`, `wheel-down`, `resize`, `truecolor` |

`Project`: `{ id, name, path, ecosystems[], branch?, default_branch?, tasks: Task[], warnings?[], hints?[], detected_from?[], member_of? }`

- `member_of`: `{ root, path, from }` when a monorepo root declares this project as a member:
  `root` is the root's id, `from` the declaration (`package.json workspaces`,
  `pnpm-workspace.yaml`, `Cargo.toml [workspace]`, `deno.json workspace`, `go.work`, `pyproject.toml [tool.uv.workspace]`,
  `settings.gradle`, `settings.gradle.kts`, or a `.sln` file name). Exclusions (`!pattern`,
  `exclude`) are honoured; when two roots claim a project, the nearest wins.

- `branch`: the checked-out branch, or the short SHA when HEAD is detached, read from `.git/HEAD`
  of a repository root (a package of a monorepo has none). `default_branch`: the remote's default
  branch from `refs/remotes/origin/HEAD`, present when the clone recorded it. Both are read from
  files; git is never run.

- `detected_from`: `{ ecosystem, from }` per ecosystem: the manifest that made it detected,
  and for JavaScript what chose the package manager (`packageManager: pnpm@9.1.0`,
  `../../pnpm-lock.yaml` for a monorepo package, `no lockfile (npm by default)`).

- `ecosystems`: `node`, `deno`, `rust`, `go`, `python`, `make`, `just`, `task`, `mise`,
  `gradle`, `maven`, `dotnet`, `php`, `ruby`, `compose`, `devcontainer`, `scripts`, `procfile`, `vscode`, `ci`, `nx`, `turbo`, `readme`, or `git` (a
  repository without a known manifest; it has no tasks).
- Compose tasks: `compose:up`, `compose:down`, `compose:logs`, `compose:ps` for the stack, and
  per service `<service>:up`, `:stop`, `:restart`, `:logs`, `:sh`; a service behind a profile gets
  `--profile <first profile>`. Dev container tasks: `devcontainer:up`, and each lifecycle command
  as `devcontainer:<onCreate|updateContent|postCreate|postStart|postAttach>` run inside the
  container through `devcontainer exec` (`devcontainer:initialize` runs on the host); the object
  form gives one task per entry (`devcontainer:postAttach:server`), a named configuration under
  `.devcontainer/<name>/` prefixes them (`devcontainer:<name>:up`). String commands follow the
  shell rule (`shell`).
- Script tasks (`scripts`): files in `scripts/`, `bin/` and `tools/` that start with a shebang or
  have a script extension (`.sh`, `.bash`, `.zsh`, `.ps1`, `.py`, `.rb`, `.js`, `.mjs`, `.cjs`,
  `.ts`, and `.cmd`/`.bat` on Windows); named after the file without its extension, `runner` is
  the folder, `command` is the interpreter then the path (`["bash", "scripts/deploy.sh"]`). They
  add tasks to a project or a git repository and never make a project on their own.
- `Procfile` processes (`procfile`), `.vscode/tasks.json` (`vscode`: `process` tasks as an argv,
  `shell` tasks as a line, the platform override first, tasks using VS Code variables other than
  `${workspaceFolder}` left out) and CI jobs (`ci`): one task per GitHub Actions job
  (`ci:<workflow>:<job>`, its `run:` steps with `working-directory`) or GitLab CI job (`ci:<job>`,
  `before_script` then `script`), run as `set -e` then the lines; the description counts the steps
  that cannot run locally (`uses:`, a non-POSIX `shell:`). Same rule as script folders: they never
  make a project on their own.
- Nx (`nx`): each target of `project.json`, plus the plain target names of `nx.json`
  `targetDefaults`, run as `npx nx run <project>:<target>`; targets inferred by Nx plugins are not
  listed (they exist only once Nx runs). Turborepo (`turbo`): each key of `tasks` (turbo 2) or
  `pipeline` (turbo 1) as `turbo:<task>`, run as `npx turbo run <task>`; `web#build` adds
  `--filter web`, `//#lint` is the root package's task.
- README steps (`readme`, `suggested: true`): commands of the POSIX shell code blocks (`sh`, `bash`,
  `console`, `zsh`, or no language) under a heading such as Getting started, Quick start,
  Installation, Setup, Usage, Development, Running, Build or Test, in order. `$ ` prompts are
  removed and, in a block that uses them, output lines dropped; `\` continuations are joined;
  trailing `# comments` are dropped; `git clone` and `cd` are left out. Named `readme:` plus the
  first words (`readme:npm-run-dev`). Like script folders, never a project on their own.
- `warnings`: manifest problems (invalid JSON, wrong types). The project is still listed.
- `hints`: useful facts that are not errors, e.g. `dependencies not installed: run \`pnpm install\``.

`Task`: `{ name, runner, command: string[], description?, hooks?[], params?[], shell?, risky?, suggested?, url? }`.
`url`: where the dev server the task starts will listen (`http://localhost:5173`), worked out
from files before it runs: a `--port`/`-p` in the command, the Vite config, `PORT` in `.env`, the
published `ports` of a Compose service, then the tool's documented default (Vite, Next, Nuxt,
Astro, Remix, SvelteKit, webpack, Storybook, Angular, Gatsby, Hugo, Jekyll, Django, Flask, uvicorn,
FastAPI, `http.server`, Rails, Laravel, MkDocs). The task's output is never read.
`shell: true`: the command is `["sh", "-c", <line>, "sh"]`, a line from a file that needs a shell;
extra arguments become `$1`, `$2`… `risky: true`: the name looks destructive (config `risky`). Both
`suggested: true`: a step taken from the README (prose, not a tool's declaration). All three
are confirmed before they run: without a terminal, pass `--yes`, or the run fails with exit 2 and
`error.kind` `confirmation-required`.
`params`: `{ name, form }[]`, the input the task cannot run without, read statically: just
recipe parameters without a default (`positional`, or `variadic` for `+name`), Taskfile
`requires: vars` (`variable`, passed as `NAME=value`), and scripts that are only a command
prefix such as `pnpm --filter`, `turbo run` or `npx` (`variadic` `command`/`script`/`task`, or a
trailing `--filter`-like flag missing its value). Direct mode refuses to run such a task without
arguments: exit 2, `error.kind` `missing-input`; `run -n` still prints the command. `command` is an
argv, never a shell string. `hooks` lists the `pre`/`post` scripts the runner executes
around the task.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Internal error, the task could not be started, or a confirmation was declined |
| 2 | Usage error (bad flags, missing task, invalid value), a task with `params` run without arguments (`missing-input`), or a `shell`/`risky` task without `--yes` and no terminal (`confirmation-required`) |
| 3 | Project, task or alias not found (`suggestions` filled) |
| 4 | Ambiguous name (`suggestions` lists the candidates) |
| 5 | Invalid configuration file |
| 127 | Task runner not installed (`suggestions` holds the install command) |
| 130 | Interactive setup cancelled |
| other | `kw run` mirrors the task's exit code (128 + signal on Unix) |

## Name resolution

Exact → case-insensitive → unique prefix, on the project id (`apps/web`) and its folder
name (`web`). Fuzzy matching is used **only** for suggestions, never to pick what runs.
Tasks accept `runner:name` to disambiguate (`make:build`). Aliases are checked before
projects in direct mode (`kw <alias>`).

## Arguments

Everything after `--` is passed to the task unchanged, without a shell. kiwa adds the
separator each runner needs (see [explanation/args-forwarding.md](../explanation/args-forwarding.md)).
