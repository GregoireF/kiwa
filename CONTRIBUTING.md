# Contributing to kiwa

Thanks for helping! Every level of experience is welcome. Issues labelled
`good first issue` are small and self-contained; adding a detector for your favourite tool
is a good first pull request ([how](docs/explanation/architecture.md#adding-a-detector)).
Say on the issue that you are taking it, to avoid two people writing the same change.

## Setup

```sh
cargo build
cargo run -p kiwa -- -w examples/demo      # the picker on the demo workspace
cargo xtask sandbox                        # install this checkout into a throwaway HOME and run it
```

## Before asking for review

```sh
cargo xtask check        # fmt, clippy -D warnings, tests, 90 % line coverage (with cargo-llvm-cov)
cargo xtask bench        # performance budgets on the release binary
cargo xtask snapshot     # the picker as text, for any visual change
cargo xtask demo         # regenerate docs/demo/demo.gif from docs/demo/demo.tape (needs Docker)
```

A change that shows in the demo (keys, layout, wording) updates `docs/demo/demo.tape` and
regenerates the GIF in the same pull request.

`cargo test -p kiwa --test real_pm` runs npm, pnpm, Yarn 1, Yarn 4 and Bun for real; tools
you do not have are skipped locally and required in CI (`KIWA_REQUIRE_RUNNERS=1`).

## Conventions

The repository follows the conventions of [ship-it](https://github.com/GregoireF/ship-it):

- **Titles and commits:** gitmoji + Conventional Commits type, imperative, subject ≤ 50
  characters: `✨ feat: group monorepo packages`, `🐛 fix: keep hooks for pnpm 8`,
  `📝 docs: …`, `✅ test: …`, `♻️ refactor: …`, `👷 ci: …`. Pull requests are squash-merged:
  CI lints the PR title, since it ends up as the commit message.
- **Sign-off:** every commit carries a [Developer Certificate of Origin](https://developercertificate.org)
  sign-off: `git commit -s`.
- **License headers:** new files start with `SPDX-License-Identifier: MIT OR Apache-2.0`.
- **Pull request template:** fill in how you tested, in the first person, and the AI usage.

## Rules

- Detectors are static: never spawn a process during discovery.
- Tasks run as an argv, never through a shell.
- Behaviour that depends on another tool is measured or sourced, and tested (`real_pm.rs`).
- Changes to `--json` output, flags, env vars, config keys or exit codes update
  [the reference](docs/reference/json-and-exit-codes.md), [stability](docs/reference/stability.md)
  and [CHANGELOG.md](CHANGELOG.md).
- Each new dependency is explained in its pull request: what it saves us from writing, and why that code is hard.
- Design decisions get an [ADR](docs/adr/README.md).
- Using AI assistants is fine; you remain responsible for what you submit.

By contributing, you agree that your contributions are dual-licensed under MIT OR Apache-2.0
and certify the DCO. Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).
