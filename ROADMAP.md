# Roadmap

Where kiwa stands and what comes next, by version. There are no dates: an item is ticked when
it is done, tested and documented. Every version has an **Exit** condition that can be
measured; it is checked before the version is released.

Each item has an issue reference (`K-nn`). Until the repository is public the issues are
drafted locally; they keep the same reference once created on GitHub.

**North star:** the fastest way for a developer *or an agent* to find and run anything in any
project on a machine, safely, with no setup.

## Principles (every version)

- **Works out of the box.** kiwa finds projects and tasks with no configuration; settings exist for
  preferences, and kiwa writes them itself.
- **Predictable.** kiwa never loses data, never waits for input when no one is at the terminal, reports
  every failure with a non-zero exit code, and only runs a task the user explicitly picked: a fuzzy
  match narrows the list, it never decides.
- **Measured, not assumed.** Behaviour that depends on another tool is measured or sourced
  ([example](docs/explanation/args-forwarding.md)); performance has budgets checked in CI.
- **Two complete input methods.** Whatever the mouse can do, the keyboard can do too, and every
  control drawn on screen reacts to a click.
- **Tested and documented on arrival.** A feature lands with its tests and its docs; CI fails below
  90 % line coverage.
- **Local-first.** kiwa never makes a network call.
- **Few dependencies.** A crate earns its place only when writing the same code ourselves would be
  long or error-prone.
- **A shell line runs only once it has been shown.** Tasks run as an argv. A line that needs a
  shell (`&&`, pipes, redirections) runs through a named shell after it is displayed and confirmed.

## 0.1: find and run anything

**For:** a developer who just cloned a repository they do not know, a developer juggling many
repositories in many stacks, and a beginner who does not yet know npm from pnpm or make from just.

**Exit:** on Windows, macOS and Linux CI, with real npm, pnpm, Yarn 1, Yarn 4 and Bun: every task of a
polyglot workspace of 60 projects and 190 tasks is reached in at most 5 keystrokes plus Enter, 3 on average; start-up is
under 25 ms and a 200-project workspace scans in under 150 ms; line coverage ≥ 90 %; the reference
docs list every flag, environment variable, config key, JSON shape and exit code.

Discovery
- [x] Static detection of 15 ecosystems plus plain git repositories; nested projects; symlinks and junctions; `.gitignore`
- [x] Package manager from `packageManager`, then the nearest lockfile up to the scan root
- [x] Hooks folded only when the runner runs them; argument separator per runner, both measured
- [x] BOM-tolerant manifests; broken manifests listed with a warning; "dependencies not installed" hint
- [x] K-04 Show why a runner was chosen (`pnpm · pnpm-lock.yaml`) in the details and in JSON
- [x] K-17 Corepack-aware install hint (Corepack is not bundled with Node.js 25 and later)
- [x] K-32 Monorepo members from what each ecosystem declares: `workspaces` (npm, Yarn, Bun),
  `pnpm-workspace.yaml`, Cargo `[workspace] members` / `exclude`, `go.work`, uv
  `[tool.uv.workspace]`, Gradle `settings.gradle(.kts)`, .NET `.sln`; exclusions honoured, folder
  nesting only as a fallback; the details and JSON say which file made a project a member
- [x] K-33 Tasks that take input: just recipe parameters without a default, Taskfile
  `requires: vars`, and scripts that are only a command prefix (a workspace fan-out or a runner
  with flags and no command); the picker opens the argument prompt pre-labelled with what is
  missing, direct mode exits 2 and names it, JSON lists it as `params`
- [x] K-37 Containers as tasks: per Compose service `up`, `stop`, `logs -f`, `exec sh` and `restart`,
  plus `up`/`down` for the whole stack, profiles honoured; `devcontainer.json` lifecycle commands
  (`onCreateCommand`, `postCreateCommand`, `postStartCommand`, …) listed as tasks and run inside the
  container through the `devcontainer` CLI, with `devcontainer:up` to create it
- [x] K-38 Scripts folders: executable files in `scripts/`, `bin/` and `tools/` (a shebang, or
  `.sh`, `.ps1`, `.py`, `.cmd`) listed as tasks, run with the interpreter their shebang or extension names
- [x] K-39 Editor and CI tasks: `.vscode/tasks.json`, `Procfile` processes, and the `run:` steps of
  GitHub Actions and GitLab CI jobs (one task per job, to reproduce CI locally); `shell` ones follow K-71
- [x] K-70 Workspace task graphs: Nx project targets (`project.json`, `nx.json` target defaults),
  `turbo.json` tasks run through `turbo run`, Deno workspace members' tasks
- [x] K-71 Shell lines, explicit and confirmed: a line without shell operators is split into an argv
  as today; any other runs through a named shell (`sh`, `pwsh` on Windows) only after it is shown and
  confirmed; a `shell` badge in the list and `shell: true` in JSON; ADR 0009 and the AGENTS.md rule
  updated; measured on the real shells like the runners
- [x] K-72 README quick-start: shell code blocks under headings such as Getting started,
  Installation, Usage or Development listed as suggested steps, marked `readme`, always confirmed
  (K-71), never run by `setup` without being shown

Onboarding
- [x] K-73 `kiwa setup [project]`: an ordered plan to get a project running, shown before anything
  runs: install dependencies with the detected manager (`npm ci` when a lockfile exists, `pnpm install`,
  `uv sync`, `bundle install`, `composer install`, `go mod download`…), copy `.env.example` to `.env`
  when absent (never overwrite), then declared setup tasks (`setup`, `bootstrap`, `prepare`, the dev
  container's `postCreateCommand`); each step confirmed, `--yes` for all, `--dry-run` and JSON plan;
  in the picker as the first entry of a project that is not set up
- [x] K-74 `kiwa about <project>` and the details pane: what the project is (name and description
  from its manifests or the README's first sentence, licence, repository URL, branch, stack) and how
  to start (the install, dev, test and build tasks recognised, the `setup` plan, required versions from
  `.nvmrc`, `engines`, `rust-toolchain.toml`, `.python-version`, `go.mod`, `.tool-versions`); `--json`
- [x] K-75 Ports and URLs, worked out from files: a `--port` in the command, the Vite config, `PORT`
  in `.env`, Compose `ports`, then the tool's documented default; the URL is printed before the run
  (clickable in most terminals), shown in the details and opened with Ctrl+O; a port already in use is
  reported before the run with the task that last used it. The task's output is never read

Picker (UX and UI)
- [x] One fuzzy search over projects × tasks, underlined matches, last result per task, details pane
- [x] One-line header with the scope as a path, faint grouping of repeated names, a scrollbar that doubles as the divider, explicit empty states
- [x] Run with arguments, run again, back to the picker with the last result, project-scoped mode
- [x] K-01 Escape clears the search, then leaves the project scope, then quits; ↑ and ↓ wrap around the list
- [x] K-02 Buttons: `Folders` and `Quit` in the header, reached with Tab; the footer's `⏎ run`, `→ args` and `esc` entries are clickable; every button has a key
- [x] K-03 `recent` group at the top (tasks run in the last 14 days); a scoped picker starts on the task run last
- [x] K-05 Mouse details: clicks count once the pointer has moved after the picker opens, right-click asks for arguments, clicking the scrollbar track moves a page
- [ ] K-06 `kw doctor`: what kiwa sees on this machine (terminal, colours, mouse, runners on `PATH`, config and state paths), with `--json`;
  `kw doctor --probe` checks clicks, motion, wheel, colours and resizing interactively, and the results for ten
  terminals (Windows Terminal, conhost, VS Code, mintty, macOS Terminal, iTerm2, GNOME Terminal, Konsole, Kitty,
  WezTerm) are published in the reference
- [x] K-07 Column widths measured in terminal cells (emoji, CJK)
- [x] K-08 Monorepo packages grouped under their root (indent, ←/→ collapse and expand)
- [x] K-09 Coloured `--help`; `FORCE_COLOR` honoured for the lines printed around a run
- [x] K-10 `→` runs with arguments (Tab moves to the buttons)
- [x] K-34 Workspace switcher behind `Folders`: named workspaces with their project counts, missing
  folders flagged, a new one added from a path; an `all` entry searches every workspace at once, and
  `@name` in the search narrows to one workspace without leaving the picker
- [x] K-35 Branch column in the list, read from `.git/HEAD` without running git, shown when the
  terminal is wide enough; the default branch is faint so that feature branches stand out

Configuration
- [x] KDL v2, edited in place with comments kept; `kw init` with project counts; aliases; `config set`; `reset`
- [x] K-11 Versioned config (`version 1`), automatic migration with a `config.kdl.bak` backup
- [x] K-12 `kw config get <key>` and `kw config edit` (`$VISUAL`, `$EDITOR`, then the OS default)
- [x] K-13 Named workspaces (`workspace "work" { roots … }`), `-W <name>`, `KIWA_WORKSPACE` (switching from the picker: K-34)
- [x] K-36 `kw shell-init --name <cmd>`: the picker under a command name of your choice in every
  supported shell, refused when it would shadow an existing command, removed by dropping the line
- [x] K-14 Hide tasks by pattern (`hide "*:lint:fix"`), show or hide git repositories without tasks
- [x] K-15 Config file with a byte order mark accepted

Robustness and safety
- [x] Exit code mirrored, named signals, Ctrl+C status on Windows
- [x] K-76 Risky tasks confirmed: names like `deploy`, `publish`, `release`, `drop`, `prune`,
  `destroy`, `reset` or ending in `:prod` ask `y` in the picker and need `--yes` in direct mode
  (exit 2, `confirmation-required`, when no one is at the terminal); the list is a config key
  (`risky`); the agents' trust policy stays K-42
- [x] K-16 Signals forwarded to the task and no child left behind when kw or its terminal goes away (process group on Unix, job object on Windows)

Quality
- [x] 200+ tests, ≥ 90 % line coverage, clippy `-D warnings`, MSRV 1.88
- [x] K-18 Real package-manager tests in CI (npm, pnpm, Yarn 1, Yarn 4, Bun), none skipped there
- [x] K-19 Keystroke budget test on a polyglot workspace of 190 tasks (measured: worst 5 + Enter, mean 2.9 + Enter)
- [x] K-20 Performance budgets (`cargo xtask bench`) enforced in CI
- [x] K-21 The picker renders without overflow at every width from 60 to 200 columns, in colour and monochrome
- [x] K-22 Fuzzing of the manifest parsers (Makefile, justfile, Taskfile, Rakefile, KDL)
- [x] K-23 `cargo xtask check | bench | snapshot | sandbox`, the same commands CI runs

Repository and release
- [x] dist release workflow: 7 targets, shell and PowerShell installers, CycloneDX SBOM, build provenance
- [x] K-24 Workflows: CI with a single required "CI passed" check, real-PM, zizmor and actionlint, PR title, labels, triage, DCO, stale, CodeQL, dependency review, Scorecard, pinned-tool watch, nightly, fuzz
- [x] K-25 Issue forms, pull request template, CODEOWNERS, SUPPORT, labels, Dependabot with a 7-day cooldown
- [x] K-26 Homebrew formula and Scoop manifest published to `GregoireF/homebrew-tap` on each release
- [x] K-77 The command is `kiwa`, with `kw` beside it where it is free: the shell and PowerShell
  installers, Homebrew and Scoop install both, and so will winget, npm and PyPI (K-78); `.deb`, `.rpm` and AUR install
  `kiwa` only, because Debian's `kworkflow` ships `/usr/bin/kw`; `kiwa shell-init --name kw` adds it
  when nothing else is called `kw` (K-36); docs and examples say which name they use
- [x] K-27 Publish 0.1.0: public repository, GitHub release with installers, Homebrew tap, Scoop bucket and
  crates.io; the two-week release-candidate period applies before 1.0

Documentation
- [x] K-28 Docs organised as tutorials, how-to guides, reference and explanation; design decisions as public ADRs
- [x] K-29 Stability reference: every flag, environment variable, config key, JSON shape and exit code
- [x] K-30 Troubleshooting guide
- [x] K-31 Animated demo generated from a script in the repository (VHS in Docker, `cargo xtask demo`)

## 0.2: agents

**Exit:** an agent in Claude Code, Codex or Cursor discovers and runs a task through MCP with no
documentation, and can never run a destructive task without the user's confirmation.

- [ ] K-40 `kw mcp`: MCP server over stdio (`list_projects`, `get_project`, `run_task`, dry-run by default)
- [ ] K-41 `kw agent skill`: install the agent skill for Claude Code, Codex, Cursor and Copilot
- [ ] K-42 Trust policy: per-root allowlist; destructive task names (`deploy`, `drop`, `prune`, …) need confirmation
- [ ] K-43 `kw agent context <project>`: write an `AGENTS.md` from what was detected
- [ ] K-44 Task logs kept per run, readable by agents (`kw logs`)

## 0.3: many repositories

**Exit:** the git state of 100 repositories is shown in under 300 ms, and a command runs across a
filtered set of them in parallel with grouped output.

- [ ] K-50 `kw status`: dirty, ahead/behind, stale branches across every repository
- [ ] K-51 `kw run --all`: a task or command across projects, in parallel, grouped output
- [ ] K-52 Tags and filters (`--stack rust`, `--tag work`, `--dirty`)
- [ ] K-53 Workspace manifest: clone and sync a set of repositories from KDL
- [ ] K-54 Background tasks (`kw ps`, `kw stop`)

## 0.4: extensibility and reach

**Exit:** on every install channel, a fresh CI machine installs kiwa with one command and runs
`kiwa --version` (K-79).

- [ ] K-78 More install channels, each in one command: `npx kiwa` / `npm i -g kiwa` (prebuilt binaries
  through per-platform optional dependencies), `uvx kiwa` / `pipx install kiwa` (binary wheels),
  `winget install GregoireF.kiwa`, `.deb` and `.rpm` on the release, AUR `kiwa-bin`, a Nix flake
  (`kiwa` checked free on npm, PyPI and crates.io on 2026-11-13)
- [ ] K-79 Every channel verified in CI on each release: a fresh runner per OS installs with the
  documented command and runs `kiwa --version` and `kiwa list --json` on `examples/demo`
- [ ] K-60 Detectors declared in KDL (file → tasks), no code
- [ ] K-61 Sandboxed plugin detectors (WASM)
- [ ] K-62 More package channels: Chocolatey, an apt and a dnf repository, an OCI image and a Dev
  Container Feature
- [ ] K-63 Podman and `podman-compose` when Docker is absent (K-37 covers Docker)

## 1.0: stable

Semantic-versioning promise on command names, flags, environment variables, config keys and
location (with automatic migration), exit codes and the `kiwa/v1` JSON shapes. Released after a
two-week release-candidate period without an interface change.

**Exit:** on a corpus of 20 well-known open-source repositories, every expected task is found (K-80).

- [ ] K-80 Real-repository corpus: 20 well-known open-source repositories pinned by commit (a Next.js
  app, Django, Rails, a Turborepo and an Nx monorepo, a Go service, a Rust workspace, a Compose stack,
  a dev container, a uv workspace, a Gradle and a .NET solution…), fetched by a nightly job; a
  checked-in expectation file fails when an expected task or member is missing; `cargo xtask corpus`
  runs it locally

## After 1.0

- **Browser interface** (`kw ui`): local only. Anything that executes commands over HTTP is a target
  for other pages open in the browser, so: 127.0.0.1 on a random port, a random per-session token in the opened URL and on every
  request, `Host` and `Origin` checked, only tasks kiwa detected can run, the server stops with kw.
- Optional i18n of the interface.

## Not planned

- **Build caching and task graphs:** Nx, Turborepo, moon and Bazel do it; kiwa runs them.
- **Toolchain installation:** mise, asdf and devbox do it; kiwa detects them.
- **Update checks or telemetry:** kiwa makes no network call; package managers announce updates.
- **A default package manager setting:** detection is correct by construction; a default would
  run lockfile projects with the wrong tool.
