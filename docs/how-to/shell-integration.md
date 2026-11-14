# Shell integration

One line in your shell profile gives you:

- **`kcd <project>`**: jump into a project's folder (fuzzy-resolved like every kw name);
- **completions** for every `kw` command and flag;
- **one shell command per alias** from your config (`alias "t" "api" "test"` → type `t`).

| Shell | Add to your profile |
|---|---|
| bash (`~/.bashrc`) | `eval "$(kw shell-init bash)"` |
| zsh (`~/.zshrc`, after `compinit`) | `eval "$(kw shell-init zsh)"` |
| fish (`~/.config/fish/config.fish`) | `kw shell-init fish \| source` |
| PowerShell (`$PROFILE`) | `Invoke-Expression (& { (kw shell-init powershell \| Out-String) })` |
| elvish (`~/.config/elvish/rc.elv`) | `eval (kw shell-init elvish \| slurp)` |
| nushell | `kw shell-init nushell \| save -f ~/.kiwa.nu`, then `source ~/.kiwa.nu` in `config.nu` |

This is the pattern used by zoxide and starship: kw prints shell code, the shell evaluates
it at start-up. Nothing is written to your PATH, no admin rights are needed, and removing
the line removes everything.

Only completions? `kw completions <shell>` prints just the completion script.

## The picker under your own name

`--name <cmd>` also defines `<cmd>` as the picker (it runs `kw` with the same arguments), with
its own completions in bash, zsh, fish and elvish:

```sh
eval "$(kw shell-init bash --name kp)"   # kp → the picker, kp api test → runs it
```

kw refuses a name that would hide something else, and exits 2 saying what: a builtin, keyword or
default alias of a supported shell (`cd`, `ls`, `rm`, `fn`, …), a program on `PATH`, one of your
kiwa aliases, or `kiwa`/`kcd`. Nothing is written anywhere: remove the line to remove the command.

Installed from a Linux distribution package, the program is `kiwa` only (Debian's `kworkflow`
already ships `kw`). `eval "$(kiwa shell-init bash --name kw)"` adds the short name, unless a `kw`
is already on `PATH`. The functions and completions call the program by the name that generated
them.

## Why `kcd` is a shell function

A program cannot change the current directory of the shell that started it. `kw path
<project>` prints the folder; the `kcd` function, running inside your shell, does the `cd`.

## Aliases

```sh
kw alias add t api test            # kw t      → kw api test
kw alias add w web dev -- --open   # kw w      → kw web dev -- --open
kw alias list
kw alias rm t
```

Aliases live in the config file (`alias "t" "api" "test"`), so they are versionable and
shareable. Alias names are letters, digits, `-` and `_`, and cannot shadow a kw command.
