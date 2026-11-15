# Your first five minutes with kw

In five minutes you will install kiwa, point it at your projects, run a task, and make the next
run a single key.

## 1. Install

```sh
brew install gregoiref/tap/kiwa                       # macOS, Linux
scoop bucket add gregoiref https://github.com/GregoireF/homebrew-tap
scoop install kiwa                                     # Windows
cargo install kiwa --locked                            # any platform, Rust 1.88+
```

This installs the program twice, as `kiwa` and as the short name `kw` used below. Check it with
`kw --version`.

## 2. Choose your folders

Run `kw` anywhere. If there is nothing to run where you are and no configuration yet, kw
offers the setup. It does not guess folder names: it finds the folders where your projects
actually sit (around where you are, under your home folder, and on Windows on your other
drives) and lists them **with how many projects and tasks each holds**, busiest first. Tick
one or more with space and press Enter: that is the whole setup. Nothing is written until you
press Enter. The accent colour is optional: `kw config set accent violet`.

You can do it again later with `kw init`, or from the `Folders` button in the picker, which also
switches between workspaces and adds a folder as a new one.

## 3. Find and run a task

Type a few letters of the project **and** of the task, in one search: `api te` finds the `test`
task of `api-gateway`. ↑ and ↓ move the selection. The details on the right show the exact command, why kw chose that
runner (`pnpm · pnpm-lock.yaml`) and the last result. Press Enter to run it.

The output stays on your terminal. Then: Enter goes back to the picker, `r` runs the task
again, `q` quits.

## 4. Make the next run one key

Open `kw` again: the task you just ran is at the top, under **recent**, with its result
(`✔ 2 min ago`). Enter runs it. From a script or another terminal: `kw last`.

## 5. Go further

- Run without the picker: `kw api-gateway test -- --verbose` (arguments after `--`).
- Jump into a project: add `eval "$(kw shell-init bash)"` to your shell profile, then `kcd api`
  ([shell integration](../how-to/shell-integration.md)).
- Hide noise: `kw config set hide "*:lint:fix"` ([configuration](../reference/configuration.md)).
- Start a project you just cloned: `kw about <project>` says what it needs, `kw setup <project>`
  installs it, after showing the plan.
- Every command and key: [commands and keys](../reference/cli.md).
