<div align="center">

# kiwa 🥝

**Find any project. Run any task.**<br>
One fuzzy search over every task of every project on your machine, for you and for your AI agents.

[![CI](https://github.com/GregoireF/kiwa/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/GregoireF/kiwa/actions/workflows/ci.yml)
[![CodeQL](https://github.com/GregoireF/kiwa/actions/workflows/codeql.yml/badge.svg?branch=main)](https://github.com/GregoireF/kiwa/actions/workflows/codeql.yml)
[![Release](https://img.shields.io/github/v/release/GregoireF/kiwa?include_prereleases&sort=semver)](https://github.com/GregoireF/kiwa/releases)
[![crates.io](https://img.shields.io/crates/v/kiwa?logo=rust)](https://crates.io/crates/kiwa)
[![Downloads](https://img.shields.io/github/downloads/GregoireF/kiwa/total)](https://github.com/GregoireF/kiwa/releases)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/GregoireF/kiwa/badge)](https://scorecard.dev/viewer/?uri=github.com/GregoireF/kiwa)
<br>
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange?logo=rust)](Cargo.toml)
[![docs.rs](https://img.shields.io/docsrs/kiwa-core?label=docs.rs%20kiwa-core)](https://docs.rs/kiwa-core)
[![Good first issues](https://img.shields.io/github/issues/GregoireF/kiwa/good%20first%20issue?label=good%20first%20issues&color=7057ff)](https://github.com/GregoireF/kiwa/issues?q=is%3Aopen+label%3A%22good+first+issue%22)

[Install](#install) · [Quick start](docs/tutorials/first-run.md) · [Documentation](docs/README.md) · [Roadmap](ROADMAP.md) · [Contributing](CONTRIBUTING.md)

<img src="docs/demo/demo.gif" alt="kiwa: the picker on a polyglot folder, a fuzzy search that runs a task, the workspace switcher, a project card, a setup plan and the JSON an agent gets" width="880">

</div>

## Why I built it

My dev folder holds dozens of projects, and no two start the same way: `pnpm dev`, `npm run
dev`, `cargo run`, `just watch`, `./gradlew bootRun`, `uv run`, `make plan`. Before running
anything, I `cd` in, open the manifest, look at the lockfile to guess the package manager, and
try to remember the script's name. A minute each time, ten times a day.

Most tools that help know one ecosystem: they read `package.json` and stop there. Coding agents
now guess the same way, with less context: they run `npm` in a pnpm project, pass arguments the
wrong way, or call a task that does not exist.

kiwa reads every manifest it knows, without running any of them, and gives one list of
everything runnable on the machine: the exact command of each task, and why it chose that tool.
You find a task by typing a few letters of the project and of the task, together. Agents get the
same list as JSON, with a contract they can rely on.

## In 30 seconds

<table>
<tr><th>Without kiwa</th><th>With kiwa</th></tr>
<tr><td>

```sh
cd ~/dev/shop/apps/storefront
cat package.json          # dev? start? serve?
ls *lock*                 # pnpm or npm?
pnpm run dev
cd ../../../billing
cat Taskfile.yml
task migrate
```

</td><td>

```sh
kw sto dev                # project and task, one search
kw bil mig
kw last                   # run it again
```

</td></tr>
</table>

## What it does

- **Every ecosystem, no config:** npm, pnpm, Yarn, Bun, Deno, cargo, Go, uv, Poetry, PDM,
  Hatch, make, just, Task, mise, Gradle, Maven, .NET, Composer and Rake; also Docker Compose
  services, dev containers, `scripts/` and `bin/` folders, Procfiles, VS Code tasks, GitHub and
  GitLab CI jobs, Nx targets, Turborepo tasks, monorepo packages, and plain git repositories.
- **Correct with every runner:** hooks and argument forwarding follow what npm, pnpm, Yarn and
  Bun actually do, [measured](docs/explanation/args-forwarding.md) and tested in CI with the
  real tools.
- **Gets a project running:** `kw about` says what a project is and which tool versions it
  expects; `kw setup` installs dependencies, creates `.env` from `.env.example` and runs the
  project's own setup tasks, after showing the plan.
- **Fast:** one binary that starts in about 10 ms and scans a folder of a dozen projects in
  under 20 ms; [performance budgets](CONTRIBUTING.md#4-check-it) are enforced in CI.
- **Safe:** listing never runs project code; tasks run without a shell, and a line that needs
  one (from a README or a CI job) is shown and confirmed first, like `deploy` or `db:reset`. A
  typo never runs the wrong task, and stopping kw stops the task and everything it started.
- **Built for agents too:** a versioned JSON contract with
  [documented exit codes](docs/reference/json-and-exit-codes.md), and an
  [agent skill](skills/kiwa/SKILL.md).
- **Local-first:** no network calls, no telemetry ([why](docs/adr/0007-local-first.md)).

## Install

```sh
brew install gregoiref/tap/kiwa                        # macOS, Linux
scoop bucket add gregoiref https://github.com/GregoireF/homebrew-tap
scoop install kiwa                                     # Windows
cargo install kiwa --locked                            # any platform, Rust 1.88+
```

Each [release](https://github.com/GregoireF/kiwa/releases) also has shell and PowerShell
installers, archives for 7 platforms, an SBOM and build provenance.

The program is `kiwa`, and every channel above also installs the short name `kw`, used in this
documentation. Linux distribution packages will install `kiwa` only, because Debian's `kworkflow`
already ships a `kw`; `kiwa shell-init <shell> --name kw` adds the short name when it is free.

## Use

```sh
kw                       # the picker; the first time, choose your project folders
kw api test -- -v        # run a task directly; arguments go after --
kw last                  # run the last task again
kw setup shop            # install what a project needs, after showing the plan
kw list --json           # everything runnable, for scripts and agents
```

In the picker, type a few letters of the project and of the task, then press Enter. Every key is
shown at the bottom of the screen.

```text
 kiwa   ~/dev                      14 projects · 56 tasks · scanned in 17 ms    Folders   Quit
───────────────────────────────────────────────────────────────────────────────────────────────
 ❯ api te                                                                                 1/12

● api-gateway  test    cargo                       ┃  api-gateway
  mobile-api   test    gradle                      ┃  test  cargo
  api-gateway  build   cargo           ✔ just now  ┃
  api-gateway  check   cargo                       │  $ cargo test
  api-gateway  clippy  cargo                       │  detected rust · Cargo.toml
 ↑↓ move · ⏎ run · → args · tab buttons · esc clear
```

One line in your shell profile adds `kcd <project>` to jump into a project, and completions
([shell integration](docs/how-to/shell-integration.md)).

## Documentation

- **Start:** [your first five minutes with kw](docs/tutorials/first-run.md).
- **Guides:** [shell integration](docs/how-to/shell-integration.md),
  [use kw from an AI agent](docs/how-to/use-with-agents.md),
  [troubleshoot](docs/how-to/troubleshoot.md).
- **Reference:** [commands and keys](docs/reference/cli.md),
  [configuration](docs/reference/configuration.md),
  [JSON and exit codes](docs/reference/json-and-exit-codes.md).
- **Background:** [architecture](docs/explanation/architecture.md),
  [decision records](docs/adr/README.md). Everything is in the [documentation index](docs/README.md).

## Prior art

Interactive script runners are not new, and kiwa owes ideas to those that came first:
[ntl](https://github.com/ruyadorno/ntl), [ni / nr](https://github.com/antfu-collective/ni),
[fuzz-run](https://github.com/sinedied/fuzz-run),
[npm-run-scripts](https://crates.io/crates/npm-run-scripts) and
[dev-workspace-cli](https://github.com/modulane/dev-workspace-cli) for JavaScript projects,
[fzf](https://github.com/junegunn/fzf) for fuzzy finding, and [just](https://github.com/casey/just)
and [Task](https://taskfile.dev) for task runners. kiwa's take: every ecosystem at once, static
detection, and a JSON contract for agents.

## Contributing

Contributions of every size are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) walks through the
setup, the checks and a first detector; the
[good first issues](https://github.com/GregoireF/kiwa/issues?q=is%3Aopen+label%3A%22good+first+issue%22)
are a good place to start. Questions go to
[Discussions](https://github.com/GregoireF/kiwa/discussions), security reports to
[SECURITY.md](SECURITY.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
