# Research: how package managers forward arguments and run hooks

kiwa never guesses how a runner behaves: the rules in `crates/kiwa-core/src/detect.rs`
come from the measurements and documentation below. Re-run the probe when a new major
version of a package manager ships.

## Method

A project with `"prebuild"` and `"build": "node show.js"`, where `show.js` prints
`JSON.stringify(process.argv.slice(2))`. Each package manager runs
`<pm> run build [--] <args>` for six argument shapes, with and without a leading `--`.
A script file is used instead of `node -e`, because `node -e "…" -x` makes Node itself
parse `-x`.

Measured on Windows 11, 2026-11-07: npm 10.9.2, pnpm 9.0.5, Yarn 1.22.22, Yarn 4.5.3,
Bun 1.3.14 (Yarn versions pinned through `packageManager` and Corepack).

## Argument forwarding

A ✔ means the arguments reached the script unchanged.

| Arguments | npm | npm + `--` | pnpm 9 | pnpm 9 + `--` | Yarn 1 | Yarn 1 + `--` | Yarn 4 | Yarn 4 + `--` | Bun | Bun + `--` |
|---|---|---|---|---|---|---|---|---|---|---|
| `x` | ✔ | ✔ | ✔ | ✘ `--` kept | ✔ | ✔ | ✔ | ✘ `--` kept | ✔ | ✔ |
| `-x` | ✘ dropped | ✔ | ✔ | ✘ | ✔ | ✔ | ✔ | ✘ | ✔ | ✔ |
| `--watch y` | ✘ `--watch` eaten | ✔ | ✔ | ✘ | ✔ | ✔ | ✔ | ✘ | ✔ | ✔ |
| `x -- y` | ✘ `--` dropped | ✔ | ✔ | ✘ | ✘ `--` dropped | ✔ | ✔ | ✘ | ✔ | ✔ |
| `-x -- y` | ✘ → `["y"]` | ✔ | ✔ | ✘ | ✘ → `["y"]` | ✔ | ✔ | ✘ | ✔ | ✔ |
| `"a b" 'c"d'` | ✔ | ✔ | ✔ | ✘ | ✔ | ✔ | ✔ | ✘ | ✔ | ✔ |
| **Faithful** | 2/6 | **6/6** | **6/6** | 0/6 | 4/6 | **6/6** | **6/6** | 0/6 | **6/6** | **6/6** |

**Rule used by kiwa:** a leading `--` for npm, Yarn 1 and Composer; none for pnpm, Yarn 2+
and Bun. Each choice is the only one that is faithful for all six shapes. Composer
documents its `--` requirement in [getcomposer.org/doc/articles/scripts.md](https://getcomposer.org/doc/articles/scripts.md).

A conditional rule ("add `--` for Yarn 1 only when the arguments contain `--`") gives the
same results on these six shapes; the unconditional rule is simpler and has no edge case.

## Pre/post hooks

| Package manager | Runs `prebuild` before `build`? | Source |
|---|---|---|
| npm 10.9 | yes | measured; [npm scripts docs](https://docs.npmjs.com/cli/v7/using-npm/scripts) |
| pnpm 9.0 | yes | measured; `enable-pre-post-scripts` defaults to `true` ([pnpm 9.x](https://pnpm.io/9.x/cli/run)) |
| pnpm 8 | **no** | `enable-pre-post-scripts` defaults to `false` ([pnpm 8.x](https://pnpm.io/es/8.x/cli/run)) |
| Yarn 1.22 | yes | measured |
| Yarn 4.5 | **no** | measured |
| Bun 1.3 | yes | measured (not stated in Bun's docs) |

**Rule used by kiwa:** when the package manager runs hooks, `prebuild`/`postbuild` are
folded into `build` (shown as its hooks) so they are not run twice by mistake. When it does
not (Yarn 2+, pnpm ≤ 8 or `enable-pre-post-scripts=false` in `.npmrc`), hooks stay listed
so they remain runnable. The pnpm version comes from the `packageManager` field; without
one, current pnpm behaviour (9+) is assumed.

## Yarn 2+ detection

In order of reliability:

1. the major version in `packageManager: "yarn@<major>"`;
2. the header of the nearest `yarn.lock`: a classic lockfile says so in a comment
   (`# yarn lockfile v1`), while the first entry of a Yarn 2+ lockfile is `__metadata:`
   (both measured, with Yarn 1.22.22 and 4.5.3);
3. a `.yarnrc.yml`, a file only Yarn 2+ reads, when the lockfile is empty or missing.

Otherwise Yarn 1 is assumed. When one folder holds lockfiles of several package managers,
the most recently written one decides and the details show a warning naming the others.

## Side findings

- Yarn 4 refuses to run any script before `yarn install`, which led to the "dependencies
  not installed" hint (`node_modules` or `.pnp.cjs` missing while dependencies are declared).
- Corepack refuses a package manager when a `package.json` in a **parent** folder declares a
  different `packageManager` (observed with a stray `~/package.json`). kiwa looks for
  lockfiles only up to the scan root, so its detection is not affected, but the runner can
  still be refused by Corepack.
