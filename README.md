<div align="center">

# kiwa 🥝

**Find any project. Run any task. One fuzzy search over every task of every project on your machine, for you and for your AI agents.**

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

---

## Why I built it

My dev folder holds dozens of projects, and none of them start the same way. One wants
`pnpm dev`, the next `npm run dev`, a third `cargo run`, then `just watch`, `./gradlew
bootRun`, `uv run`, `make plan`. Before running anything I `cd` in, open the manifest, look
for the lockfile to guess the package manager, and hope I remembered the name of the
script. It takes a minute, ten times a day.

The tools that help usually know one ecosystem: they read `package.json` and stop there.
And now coding agents do the same guessing, with less context: they run `npm` in a pnpm
project, pass arguments the wrong way, or try a task that does not exist.

kiwa reads every manifest it knows, without running any of them, and gives one list of
everything runnable on the machine, with the exact command each task will run and why it
chose that tool. You find a task by typing a few letters of the project and of the task,
together. Agents get the same list as JSON, with a contract they can rely on.

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
kw sto dev                # one search, project and task
kw bil mig
kw last                   # run it again
```

</td></tr>
</table>

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

## What it does

- **Every ecosystem, zero config:** npm, pnpm, Yarn, Bun, Deno, cargo, go, uv, poetry, pdm,
  hatch, make, just, Taskfile, mise, Gradle, Maven, .NET, Composer, Rake, Docker Compose (each
  service), dev containers, the scripts in `scripts/`, `bin/` and `tools/`, `Procfile`, VS Code tasks, CI jobs (GitHub Actions, GitLab CI), Nx targets and Turborepo tasks,
  monorepo packages and plain git repositories included.
- **Correct with every runner:** hooks and argument forwarding follow what npm, pnpm, Yarn and
  Bun actually do, [measured](docs/explanation/args-forwarding.md) and tested in CI with the real tools.
- **Fast:** one binary, 9 ms to start, 200 projects scanned in under 60 ms, with budgets enforced in CI.
- **Safe:** listing never executes project code, tasks run without a shell, a typo never runs
  the wrong task, and killing kw never leaves the task running.
- **Built for agents too:** a versioned JSON contract with [documented exit codes](docs/reference/json-and-exit-codes.md)
  and an [agent skill](skills/kiwa/SKILL.md).
- **Local-first:** no network call, no telemetry ([why](docs/adr/0007-local-first.md)).

## Install

```sh
brew install gregoiref/tap/kiwa                       # macOS, Linux
scoop bucket add gregoiref https://github.com/GregoireF/homebrew-tap
scoop install kiwa                                     # Windows
cargo install kiwa --locked                            # from source, Rust 1.88+
```

Or a [release archive](https://github.com/GregoireF/kiwa/releases) (shell and PowerShell
installers, SBOM, build provenance), or `cargo install --path crates/kiwa --locked` from a clone.

The command is `kiwa`. Every channel above also installs `kw`, the short name used in this
documentation. Linux distribution packages install `kiwa` only, because Debian's `kworkflow` already
ships a `kw`; add the short name yourself with `kiwa shell-init <shell> --name kw` when nothing else
is called `kw`.

## Use

```sh
kw                     # the picker; on first run, choose your project folders
kw api test -- -v      # run a task directly, arguments after --
kw last                # run the last task again
kw list --json         # everything runnable, for scripts and agents
```

Every command and key: [reference](docs/reference/cli.md). One line in your shell profile adds
`kcd <project>` and completions: [shell integration](docs/how-to/shell-integration.md).

## Prior art

Interactive script runners are not new, and kiwa owes ideas to the ones that came first:
[ntl](https://github.com/ruyadorno/ntl), [ni / nr](https://github.com/antfu-collective/ni),
[fuzz-run](https://github.com/sinedied/fuzz-run),
[npm-run-scripts](https://crates.io/crates/npm-run-scripts) and
[dev-workspace-cli](https://github.com/modulane/dev-workspace-cli) for JavaScript projects,
[fzf](https://github.com/junegunn/fzf) for fuzzy finding, and [just](https://github.com/casey/just)
and [Task](https://taskfile.dev) for task runners. kiwa's take: every ecosystem at once, static
detection, and a JSON contract for agents.

## Contributing

Start with [CONTRIBUTING.md](CONTRIBUTING.md) and the
[good first issues](https://github.com/GregoireF/kiwa/issues?q=is%3Aopen+label%3A%22good+first+issue%22).
Security reports go through [SECURITY.md](SECURITY.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
