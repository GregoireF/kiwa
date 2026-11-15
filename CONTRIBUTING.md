# Contributing to kiwa

Thanks for helping. Every level of experience is welcome, and so are small changes: a typo, a
clearer error message, a missing task in a detector.

- **Not sure where to start?** Issues labelled
  [`good first issue`](https://github.com/GregoireF/kiwa/issues?q=is%3Aopen+label%3A%22good+first+issue%22)
  are small and self-contained. A detector for a tool you use is a good first pull request
  ([how](#add-a-detector)).
- **Taking an issue?** Say so on the issue first, so two people do not write the same change.
- **A bigger idea?** Open a [feature request](https://github.com/GregoireF/kiwa/issues/new/choose)
  or a [discussion](https://github.com/GregoireF/kiwa/discussions) before writing code.
- **A security problem?** Never in a public issue: see [SECURITY.md](SECURITY.md).

## 1. Set up

You need [Rust](https://rustup.rs) 1.88 or later and git. Everything else is optional and only
needed by one check:

| Tool | Used by |
|---|---|
| [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) | the 90 % coverage gate of `cargo xtask check` (skipped without it) |
| Node.js with npm, pnpm, Yarn and Bun | `cargo test -p kiwa --test real_pm` (each missing tool is skipped locally) |
| Docker | `cargo xtask demo`, which regenerates the README's GIF |
| Rust nightly and [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) | the parser fuzz targets ([fuzz/README.md](fuzz/README.md)) |

```sh
git clone https://github.com/<you>/kiwa && cd kiwa    # your fork
cargo build
cargo run -p kiwa -- -w examples/demo                 # the picker on the demo workspace
cargo xtask sandbox                                   # install this checkout in a throwaway HOME and run it
```

`examples/demo` is a small polyglot workspace (npm, pnpm, cargo, uv, Gradle, Compose…) used by
the tests, the snapshot and the demo. `cargo xtask sandbox` never touches your real config or
history.

## 2. Find your way around

```text
crates/kiwa-core/   the library: detectors (detect.rs, sources.rs), discovery, name resolution,
                    config, history, setup plans
crates/kiwa/        the program: CLI (lib.rs), picker (tui.rs), init wizard, output, execution
                    (exec.rs); two binaries, `kiwa` and `kw`, with the same code
crates/kiwa/tests/  end-to-end tests of the binary (cli.rs) and of real package managers (real_pm.rs)
xtask/              developer tasks: cargo xtask check | bench | snapshot | sandbox | demo | …
fuzz/               fuzz targets for the parsers (nightly, outside the workspace)
docs/               tutorial, how-to guides, reference, explanation, decision records (ADRs)
```

[Architecture](docs/explanation/architecture.md) explains the pipeline (walk, detect, assemble,
resolve, run) in one page.

## 3. Make the change

Create a branch, then keep each pull request to one change.

- **Tests come with the change.** A detector gets a unit test; a CLI change gets a test in
  `crates/kiwa/tests/cli.rs`; a picker change gets a case in `picker_tests` (`tui.rs`).
- **The picker stays fully usable from the keyboard.** Every button has a key, and the footer
  shows it.
- **User-facing text names the program with `crate::me()`**, never a literal `kw`: it is
  `kiwa` or `kw`, whichever the user typed.
- **A visible change** (keys, layout, wording) updates `docs/demo/demo.tape` and regenerates the
  GIF with `cargo xtask demo` in the same pull request.

## 4. Check it

```sh
cargo xtask check        # what CI checks: fmt, clippy -D warnings, tests, coverage gate
cargo xtask bench        # performance budgets, on the release binary
cargo xtask snapshot     # the picker as text: paste it before and after for a visual change
```

`cargo test -p kiwa --test real_pm` runs npm, pnpm, Yarn 1, Yarn 4 and Bun for real. Tools you
do not have are skipped on your machine and required in CI.

## 5. Commit

The conventions are those of [ship-it](https://github.com/GregoireF/ship-it):

- **Title:** a gitmoji, a Conventional Commits type, then an imperative subject of 50 characters
  at most: `✨ feat: group monorepo packages`, `🐛 fix: keep hooks for pnpm 8`, `📝 docs: …`,
  `✅ test: …`, `♻️ refactor: …`, `👷 ci: …`.
- **Sign-off:** every commit carries a [Developer Certificate of Origin](https://developercertificate.org)
  sign-off. Use `git commit -s`; to fix the last commit, `git commit --amend -s --no-edit`.
- **License header:** a new source file (`.rs`, `.toml`, `.yml`…) starts with
  `SPDX-License-Identifier: MIT OR Apache-2.0` in its comment syntax. CI checks it with
  [addlicense](https://github.com/GregoireF/addlicense), which also adds missing ones:
  `addlicense --template .github/license-header.txt --ignore "target,fuzz/target,examples,docs/demo,*.md" .`

## 6. Open the pull request

Push your branch and open the pull request against `main`. The template asks for:

- the related issue (`Closes #123`);
- how you tested it, in the first person: what you ran and what you saw;
- breaking changes, and which AI tools you used, if any.

CI checks the title (pull requests are squash-merged, so it becomes the commit message), the
sign-offs, the workflows, and the tests on Linux, macOS and Windows. A maintainer reviews within
a few days; if CI fails and the reason is unclear, ask on the pull request.

## Add a detector

A detector turns a manifest into tasks, by reading files only.

1. **Add the ecosystem** to `Ecosystem` in `crates/kiwa-core/src/model.rs`. The compiler then
   points to every `match` that needs it (its name, label and runner).
2. **Write the detector** in `crates/kiwa-core/src/detect.rs`:
   `fn name(dir: &Path, root: &Path) -> Option<Detection>`. Read with `read(dir, "file")`,
   return `found(Ecosystem::Name, tasks)`, or `broken(…)` when the manifest is invalid (the
   project is then listed with a warning, never dropped). Never start a process.
3. **Register it.** A detector keyed by a manifest goes in `DETECTORS` (the order is the display
   order) and its files in `SOURCES`. One keyed by a folder entry (`project.json`, `turbo.json`)
   goes in `detect_dir`, behind `has("…")`, so large trees stay fast.
4. **Test it:** a unit test in `detect.rs` for the parsing, and a fixture in `examples/demo` if
   it is a common tool. A parser of free-form text also gets a fuzz target in `fuzz/`.
5. **Document it:** the ecosystem list in the README, and a `CHANGELOG.md` entry under
   `Unreleased`.

## Rules

- **Discovery is static:** detectors read files and never run a process, so listing an
  untrusted repository is safe ([ADR 0003](docs/adr/0003-static-discovery.md)).
- **Tasks run as an argv.** A line that needs a shell (from a README, a CI job, a Procfile)
  becomes `Task::from_line` and is shown and confirmed before it runs
  ([ADR 0009](docs/adr/0009-shell-lines-shown-and-confirmed.md)); never build a shell command
  any other way.
- **Runner behaviour is measured.** How npm, pnpm, Yarn or Bun handle hooks and arguments
  changes only with a measurement or a source, and a `real_pm.rs` test
  ([ADR 0006](docs/adr/0006-measured-runner-behaviour.md)).
- **The contract is documented.** A change to `--json`, flags, environment variables, config
  keys or exit codes updates [the reference](docs/reference/json-and-exit-codes.md),
  [stability](docs/reference/stability.md) and [CHANGELOG.md](CHANGELOG.md).
- **Few dependencies.** A new one is explained in the pull request: what it saves us from
  writing, and why that code is hard.
- **Design decisions get an [ADR](docs/adr/README.md).**
- **AI assistants are fine;** you remain responsible for what you submit, and the pull request
  says how they were used.

## License

By contributing, you agree that your contributions are dual-licensed under MIT OR Apache-2.0,
and you certify the DCO. Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).
