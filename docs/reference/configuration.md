# Configuration

kiwa works without configuration: it scans the current directory. On the first run in a
folder without projects, `kw` offers the setup (`kw init`), which lists the usual project
folders that exist on your machine **with how many projects each contains**.

**Location** (`kw config path`):

| OS | Path |
|---|---|
| Linux | `$XDG_CONFIG_HOME/kiwa/config.kdl` (`~/.config/kiwa/config.kdl`) |
| macOS | `~/Library/Application Support/kiwa/config.kdl` |
| Windows | `%APPDATA%\kiwa\config.kdl` |

Override with `KIWA_CONFIG=/path/to/config.kdl`.

## Format: KDL v2

```kdl
// Format version, written by kiwa. A file for a newer format is refused, never misread.
version 1

// Folders scanned when no -w/--root is given. `~` is expanded.
roots "~/dev" "~/work"

// Max depth below each root (default 4).
depth 4

// Extra directory names to skip. Dependency folders (node_modules, vendor, ...) are always
// skipped; build outputs (target, dist, build, bin, obj, out) are skipped inside projects.
ignore "archive" "tmp"

// Picker accent: cyan (default), azure, indigo, violet, rose, amber, steel, or none (bold
// text, no colour). Results keep green, yellow and red to themselves.
accent "amber"

// Mouse in the picker (on by default, off automatically under mintty / Git Bash).
mouse #false

// Shortcuts: kw <name> runs <project> <task> [args...].
alias "t" "api" "test"
alias "w" "web" "dev" "--open"

// Tasks hidden everywhere, as `project:task` or `runner:task` patterns (* and ? wildcards).
hide "*:lint:fix" "legacy/*" "npm:postinstall"

// Hide git repositories in which no task was detected (shown dimmed by default).
show-git-only #false

// Words that make a task risky: confirmed before it runs (y in the picker, --yes without a
// terminal). A task is risky when one word of its name (split on : - _ . / and spaces) is listed.
// This list replaces the default one: deploy publish release drop prune destroy reset purge
// wipe nuke teardown prod production.
risky "deploy" "release" "drop" "prod"

// Named sets of folders: `kw -W work`, or KIWA_WORKSPACE=work.
workspace "work" {
    roots "~/work" "D:/clients"
    depth 3
    ignore "archive"
}
```

## Workspaces in the picker

With named workspaces, the picker scans all of them at once (plus `roots`, shown as `default`)
and opens on `default`, or on the one `-W` names. `Folders` opens the switcher: `all` (every
workspace together), each workspace with its project count and folders (a folder that does not
exist is flagged `⚠ missing`), `+ add a folder…`, and `kw init`. Adding a folder writes
`workspace "<folder name>" { roots "<path>" }` to this file (comments kept) and opens the picker on
it. Typing `@name` at the start of the search narrows to a workspace (`@wo` is enough when unique,
`@all` for every one) without leaving the picker. Commands other than the picker (`list`, `run`,
…) keep scanning only `-W`'s workspace, or `roots`.

Windows PowerShell 5.1 prepends a UTF-8 byte order mark when it saves a file; kiwa skips it.

Unknown nodes produce a warning, not an error, so newer configs keep working with older
binaries. An invalid file is an error (exit code 5) that names the file.

## Changing settings from the command line

```sh
kw config set roots ~/dev D:/work
kw config set accent violet
kw config set mouse off
kw config set hide "*:lint:fix"
kw config get roots       # one setting (add --json for agents)
kw config edit            # $VISUAL, $EDITOR, else notepad / open -t / vi; validated on close
kw config show            # effective configuration (add --json for agents)
kw init                   # interactive setup, or: kw init --use ~/dev --accent amber
kw reset                  # delete config, aliases and history (asks first; --yes to skip)
```

Every write goes through the KDL document model: **your comments and layout are kept**,
the result is validated before it is saved, and the previous file is kept as
`config.kdl.bak` (also before `kw config edit` opens the editor).

## Precedence

Command-line flags > environment (`KIWA_*`) > config file > defaults.

| Variable | Effect |
|---|---|
| `KIWA_CONFIG` | config file path |
| `KIWA_STATE` | history file path |
| `KIWA_JSON=1` | same as `--json` |
| `KIWA_WORKSPACE` | same as `-W <name>` |
| `KIWA_HOME` | home folder used by `kw init` suggestions (tests, sandboxes) |
| `NO_COLOR` | no colours anywhere |

## State

Run history (for ranking and `kw last`) lives in the platform state directory
(`kw config show` prints it). It is written atomically (write, then rename) and a corrupt
file simply resets the history. Clear it with `kw history --clear`.
