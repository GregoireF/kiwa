# ADR 0006: Runner behaviour is measured, not assumed

Status: accepted (2026-11-07)

## Context
How a package manager runs `pre`/`post` hooks and forwards extra arguments differs between
tools and between versions. A blanket rule ("every tool except Yarn 2+ runs hooks") is wrong
for pnpm 8, whose `enable-pre-post-scripts` default is `false` (it became `true` in pnpm 9):
a launcher that folds `prebuild` into `build` would hide a hook pnpm 8 never runs.

## Evidence
- Probe ([explanation/args-forwarding.md](../explanation/args-forwarding.md)): 6 argument
  shapes, with and without a leading `--`, on npm 10.9, pnpm 9.0, Yarn 1.22, Yarn 4.5, Bun 1.3.
- Hooks: npm, pnpm 9, Yarn 1 and Bun run `prebuild`; Yarn 4 does not (measured).
- pnpm documentation for 8.x and 9.x; Composer documentation for its `--`.

## Decision
- Separator: always for npm, Yarn 1 and Composer; never for pnpm, Yarn 2+ and Bun. Each is
  the only choice that is faithful for all six shapes.
- Hooks are folded when the runner runs them: npm, Bun, Yarn 1, pnpm ≥ 9 unless `.npmrc`
  sets `enable-pre-post-scripts=false`. With pnpm ≤ 8 (from `packageManager`) and Yarn 2+
  they stay listed and runnable.
- The measurements are tests: `crates/kiwa/tests/real_pm.rs` runs the real tools on every
  pull request and every night (`KIWA_REQUIRE_RUNNERS=1`), so a behaviour change in a new
  release is caught by CI, not by users.

## Consequences
+ Correct on pnpm 8, Yarn 4 and every argument shape probed.
- Without a `packageManager` field, pnpm is assumed to behave like the current major (9+).
