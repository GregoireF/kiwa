# AGENTS.md

Guidance for AI coding agents working on this repository.

## Layout

- `crates/kiwa-core`: library (detectors in `detect.rs`, discovery, resolution, config, state, suggestions).
- `crates/kiwa`: the program, in the `kiwa_cli` library (`lib.rs`: clap CLI; ratatui picker in `tui.rs`, `init.rs` wizard, theme, output, execution), and two binaries calling it: `kiwa` and `kw`. User-facing text names the program with `crate::me()`, never a literal `kw`.
- `crates/kiwa/tests`: end-to-end tests of the binary (`cli.rs`) and real package managers (`real_pm.rs`).
- `xtask`: developer tasks (`cargo xtask check | bench | snapshot | sandbox | demo | outdated | dist-generate`).
- `fuzz`: cargo-fuzz targets for the parsers (nightly, outside the workspace).
- `examples/demo`: polyglot fixture workspace.
- `docs`: Diátaxis layout; `docs/reference/json-and-exit-codes.md` is the machine contract.

## Commands

```sh
cargo xtask check                  # what CI checks
cargo xtask snapshot               # the picker as text
cargo run -p kiwa -- -w examples/demo list
```

## Rules

- Detectors stay static: never spawn processes during discovery. Tasks run as an argv. A line that needs a shell becomes `Task::from_line` (`sh -c`, `shell: true`) and is shown and confirmed before it runs (ADR 0009); never build a shell command any other way.
- Runner behaviour (hooks, argument separators) changes only with a measurement and a `real_pm.rs` test.
- Any change to `--json`, flags, env vars, config keys or exit codes updates `docs/reference/` and `CHANGELOG.md`.
- Picker changes keep every button reachable from the keyboard and are covered by `picker_tests`.
- Keep the dependency count low; justify new dependencies.
- English everywhere. Commit titles: gitmoji + Conventional Commits (`✨ feat: …`), signed off (`git commit -s`).
