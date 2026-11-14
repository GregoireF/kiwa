# Troubleshoot

For anything about the machine (terminal, colours, mouse, config file, a runner not found), start
with `kw doctor`. For a project, start with `kw info <project> --json`: it shows what kiwa detected, the exact command of each
task, the runner and why it was chosen (`detected_from`), hints and warnings.

| Symptom | Cause and fix |
|---|---|
| A project is missing | It is deeper than `depth` (default 4): `kw --depth 6 list`, or `kw config set depth 6`. It is in a `.gitignore`d or hidden folder, in `node_modules`/`vendor`, or in a build-output folder (`target`, `dist`, `build`, `bin`, `obj`, `out`) **inside** another project. Or its manifest is broken: `kw list` prints `⚠` with the error. |
| A task is missing | It is hidden by a `hide` pattern (`kw config get hide`). Or it is generated at run time (a Makefile target produced by `$(eval)`): kiwa reads files and never runs code to list tasks ([why](../adr/0003-static-discovery.md)). |
| A package is not under its monorepo in the picker, or is under the wrong one | Membership comes from what the root declares; `kw info <package> --json` shows `member_of` and the file (`from`). A root that declares members leaves the others apart on purpose (an `exclude`, a `!pattern`, or simply not listed). Gradle `projectDir` overrides are not followed. |
| The wrong package manager runs | The `packageManager` field wins, then the nearest lockfile up to the scan root; `detected_from` says which. Remove the stale lockfile or set `packageManager`. |
| `prebuild` is listed, or not | kiwa follows what the runner does ([details](../explanation/args-forwarding.md)): hooks are folded for npm, Bun, Yarn 1 and pnpm ≥ 9; listed for Yarn 2+, pnpm ≤ 8 and `enable-pre-post-scripts=false`. |
| Exit 127, "`pnpm` is not installed" | The runner is not on `PATH`. kw prints the install command; Node.js 25+ no longer bundles Corepack (`npm install --global corepack`). |
| Corepack: "This project is configured to use pnpm" | A `package.json` in a **parent** folder (often `~/package.json`) declares another `packageManager`. Remove that field or that file. |
| Yarn 4 refuses to run a script | Run `yarn install` first; kiwa hints "dependencies not installed". |
| The mouse does nothing | `kw doctor` says whether the mouse is on and why not; `kw doctor --probe` shows which events the terminal sends. Git Bash / mintty does not report mouse events to console programs: use Windows Terminal, or the keyboard ([terminals](../reference/terminals.md)). |
| Text cannot be selected while the picker is open | The picker captures the mouse: hold Shift (or Option) while dragging, or `kw config set mouse off`. |
| "config format 2; this kiwa reads up to 1" | The file was written by a newer kiwa. Upgrade kw, or restore `config.kdl.bak`. |
| The picker does not open in CI or a pipe | By design: without a terminal, `kw` lists projects, or runs the task you named. |

Still stuck? Open a [bug report](https://github.com/GregoireF/kiwa/issues/new/choose) with
`kw doctor --json` and `kw info <project> --json`.
