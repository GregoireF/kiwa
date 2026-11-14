# Commands and flags

`kw [global flags] [command]`. Without a command, in a terminal, `kw` opens the picker;
without a terminal it lists projects.

The program is installed as `kiwa` and, where the name is free, as `kw` too (see
[Install](../../README.md#install)); both are the same binary. Help, errors and suggestions use the
name you typed, and so does `shell-init`. These pages write `kw`.

## Global flags

| Flag | Env | Meaning |
|---|---|---|
| `-w, --root <DIR>` (repeatable) | | Scan these folders instead of the configured roots |
| `-W, --workspace <NAME>` | `KIWA_WORKSPACE` | Use a named workspace from the config |
| `--depth <N>` | | Maximum depth below each root (default 4) |
| `--json` | `KIWA_JSON=1` | One `kiwa/v1` JSON document on stdout |
| `--no-mouse` | | Keyboard only in the picker, for this run |
| `-y, --yes` | | Answer yes to confirmations: a shell line, a risky task (`deploy`, `db:reset`, `…:prod`), `reset`. In direct mode it may also follow the task (`kw app deploy --yes`) |
| `-h, --help`, `-V, --version` | | |

## Commands

| Command | What it does |
|---|---|
| `kw` / `kw pick` | Picker over every project × task |
| `kw <project>` | Picker limited to that project (in a terminal) |
| `kw <project> <task> [-- args]` | Run a task (direct mode) |
| `kw <alias> [-- args]` | Run an alias from the config |
| `kw run <project> [task] [-n] [-- args]` | Run a task; `-n` prints what would run |
| `kw last [-n]` | Run the most recent task again |
| `kw list [-t]` (`ls`) | List projects (`-t`: with their tasks) |
| `kw info <project>` | One project: stack, branch, tasks, why each ecosystem was detected, hints, warnings |
| `kw path <project>` | Print the project's folder (`kcd` uses it) |
| `kw init [--use DIR]… [--accent NAME]` | Choose the folders to scan; interactive without `--use` |
| `kw alias add <name> <project> <task> [-- args]` / `alias rm <name>` / `alias ls` | Shortcuts |
| `kw config path` / `show` / `get <key>` / `set <key> <values…>` / `edit` | Configuration |
| `kw history [--clear]` | Run history used for ranking |
| `kw shell-init <shell> [--name CMD]` | Shell code: `kcd`, completions, one command per alias; `--name` also defines the picker as `CMD`, refused (exit 2) when it would shadow a builtin, a program on `PATH` or an alias |
| `kw completions <shell>` | Completion script only (`bash`, `zsh`, `fish`, `powershell`, `elvish`, `nushell`) |
| `kw about [project\|folder]` | What a project is (name, description from its manifests or the README's first sentence, licence, repository, branch, stack) and how to start (`setup` if something is missing, the install/dev/test/build/lint tasks, the tool versions it pins from `.nvmrc`, `engines`, `.python-version`, `requires-python`, `rust-toolchain`, `rust-version`, `go.mod`, `.tool-versions`, `mise.toml`). The picker's details show the description, `needs` and the licence |
| `kw setup [project\|folder] [-n]` | Get a project running: install dependencies with the detected manager (`npm ci`, `pnpm install`, `uv sync`, `composer install`, `go mod download`…, at the monorepo root for a member), copy `.env.example` to `.env` (never over one), run its `setup`/`bootstrap`/`init` tasks. Shows the plan, confirms it once (`--yes` without a terminal), asks again for a shell, risky or README step, stops at the first failure. README steps are listed apart, to run one by one. In the picker, a project that is not set up starts with a `setup` entry |
| `kw doctor [--probe]` | What kiwa sees here: terminal, colours, mouse, config and state files, runners on `PATH` (no process started; works with a broken config). `--probe` checks clicks, motion, wheel, resizing and colours interactively ([terminals](terminals.md)) |
| `kw reset [-y]` | Delete the config, aliases and history (asks first) |

## Picker keys

| Key | Action |
|---|---|
| letters, digits | Search projects and tasks together; `@name` first narrows to a workspace (`@all`: every one) |
| ↑ ↓, Ctrl+P/N | Move one row, wrapping around at both ends |
| Page Up/Down, Home/End | Move a page, or to the first or last row |
| Enter | Run the selected task; a task that needs input (`params`) opens the argument prompt, labelled with what is missing; a shell line or a risky task is shown first and runs on `y` or Enter (Esc cancels) |
| → | Run with arguments (a prompt; quotes group words, no shell); on a folded monorepo, unfold it |
| ← | Fold the monorepo group of the selected row (while browsing, not while searching) |
| Tab / Shift+Tab | Move to the `Folders` and `Quit` buttons; ← → between them, Enter presses, ↓ or Esc returns |
| `Folders` | The workspace switcher: `all`, each workspace with its project count (missing folders flagged), `+ add a folder…`, `kw init`; ↑ ↓ (wrapping) and Enter, or a click; Esc goes back ([configuration](configuration.md#workspaces-in-the-picker)) |
| Esc | Clear the search, then leave the project, then quit |
| Backspace on an empty search | Leave the project |
| Ctrl+U | Clear the search |
| Ctrl+O | Open the selected task's URL in the browser (a dev server whose port is known; `^O open` in the footer) |
| Ctrl+C | Quit |
| Mouse | A click selects, a second click runs; the footer's `⏎ run`, `→ args` and `esc` entries are buttons; right-click selects a row and asks for its arguments; the wheel scrolls; clicking the scrollbar track above or below the thumb pages; clicking `▾`/`▸` folds or unfolds a monorepo |

After a run: Enter goes back to the picker, `r` runs again, `q` or Esc quits.
