// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shell integration, following the `eval "$(tool init <shell>)"` pattern popularised by
//! zoxide and starship. One line in the shell profile gives:
//! - `kcd <project>`: cd into a project (a child process cannot change its parent's cwd),
//! - completions for `kw`,
//! - one shell command per config alias (no files on PATH, no admin rights).

use crate::cli::{Cli, Shell};
use clap::CommandFactory;
use clap_complete::Shell as C;
use kiwa_core::config::Alias;

pub fn completions(shell: Shell) -> String {
    completions_for(shell, crate::me())
}

fn completions_for(shell: Shell, bin: &str) -> String {
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    match shell {
        Shell::Bash => clap_complete::generate(C::Bash, &mut cmd, bin, &mut buf),
        Shell::Zsh => clap_complete::generate(C::Zsh, &mut cmd, bin, &mut buf),
        Shell::Fish => clap_complete::generate(C::Fish, &mut cmd, bin, &mut buf),
        Shell::Powershell => clap_complete::generate(C::PowerShell, &mut cmd, bin, &mut buf),
        Shell::Elvish => clap_complete::generate(C::Elvish, &mut cmd, bin, &mut buf),
        Shell::Nushell => {
            clap_complete::generate(clap_complete_nushell::Nushell, &mut cmd, bin, &mut buf)
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Builtins, keywords and default aliases a function must not hide, across the supported
/// shells (bash, zsh, fish, PowerShell, elvish, nushell). Checked without starting a shell.
const SHELL_WORDS: &[&str] = &[
    // POSIX and bash/zsh builtins and keywords
    "alias",
    "bg",
    "bind",
    "break",
    "builtin",
    "case",
    "cd",
    "command",
    "continue",
    "declare",
    "dirs",
    "do",
    "done",
    "echo",
    "elif",
    "else",
    "enable",
    "esac",
    "eval",
    "exec",
    "exit",
    "export",
    "false",
    "fc",
    "fg",
    "fi",
    "for",
    "function",
    "getopts",
    "hash",
    "help",
    "history",
    "if",
    "in",
    "jobs",
    "kill",
    "let",
    "local",
    "logout",
    "popd",
    "printf",
    "pushd",
    "pwd",
    "read",
    "readonly",
    "return",
    "select",
    "set",
    "shift",
    "source",
    "test",
    "then",
    "time",
    "times",
    "trap",
    "true",
    "type",
    "typeset",
    "ulimit",
    "umask",
    "unalias",
    "unset",
    "until",
    "wait",
    "while",
    "autoload",
    "bindkey",
    "compdef",
    "emulate",
    "setopt",
    "unsetopt",
    "where",
    "whence",
    "which",
    // fish
    "abbr",
    "and",
    "begin",
    "contains",
    "count",
    "end",
    "funced",
    "funcsave",
    "functions",
    "math",
    "not",
    "or",
    "status",
    "string",
    // PowerShell default aliases
    "ac",
    "cat",
    "chdir",
    "clc",
    "clear",
    "cls",
    "copy",
    "cp",
    "cpi",
    "del",
    "diff",
    "dir",
    "erase",
    "fl",
    "foreach",
    "ft",
    "fw",
    "gal",
    "gc",
    "gci",
    "gcm",
    "gi",
    "gl",
    "gm",
    "gp",
    "gps",
    "group",
    "gsv",
    "gv",
    "h",
    "iex",
    "ii",
    "ipmo",
    "iwr",
    "ls",
    "man",
    "md",
    "measure",
    "mi",
    "mount",
    "move",
    "mv",
    "ni",
    "nv",
    "ogv",
    "ps",
    "r",
    "rd",
    "ren",
    "ri",
    "rm",
    "rmdir",
    "sc",
    "si",
    "sl",
    "sleep",
    "sort",
    "sp",
    "start",
    "tee",
    "wget",
    "curl",
    "write",
    // elvish and nushell
    "fn",
    "var",
    "def",
    "use",
    "each",
    "get",
    "open",
    "save",
    "mkdir",
    "touch",
];

/// Why `name` cannot be used for the picker command, or `None` when it is free. `path` is the
/// `PATH` to search (the process's own when `None`).
pub fn shadowing(
    name: &str,
    aliases: &[Alias],
    path: Option<std::ffi::OsString>,
) -> Option<String> {
    if !valid_name(name) {
        return Some(format!(
            "`{name}`: a command name uses letters, digits, - and _"
        ));
    }
    if name == "kiwa" || name == "kcd" {
        return Some(format!("`{name}` is already defined by kiwa"));
    }
    if SHELL_WORDS.contains(&name) {
        return Some(format!(
            "`{name}` is a shell builtin, keyword or default alias"
        ));
    }
    if aliases.iter().any(|a| a.name == name) {
        return Some(format!("`{name}` is one of your kiwa aliases"));
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let path = path.or_else(|| std::env::var_os("PATH"));
    let found = which::which_in(name, path, cwd).ok()?;
    // `kw` installed next to kiwa is kiwa itself: nothing to add, nothing hidden.
    let ours = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| found.parent() == Some(d)))
        .unwrap_or(false);
    Some(if ours {
        format!(
            "`{name}` is already installed with kiwa ({})",
            found.display()
        )
    } else {
        format!("`{name}` would hide {}", found.display())
    })
}

/// The picker under another name, in one shell.
fn picker_command(bin: &str, shell: Shell, name: &str) -> String {
    match shell {
        Shell::Bash | Shell::Zsh => format!("{name}() {{ command {bin} \"$@\"; }}\n"),
        Shell::Fish => format!("function {name}\n    command {bin} $argv\nend\n"),
        Shell::Powershell => format!("function {name} {{ & {bin} @args }}\n"),
        Shell::Elvish => format!("fn {name} {{|@a| {bin} $@a }}\n"),
        Shell::Nushell => format!("def {name} [...args] {{ ^{bin} ...$args }}\n"),
    }
}

/// Shell-safe single-quoted word (POSIX shells, fish, PowerShell all accept '...').
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn ps_q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Shell code: `kcd`, alias commands, completions, and with `name` the picker under that name
/// (with its own completions where the shell can attach them to a function).
pub fn init(shell: Shell, aliases: &[Alias], name: Option<&str>) -> String {
    init_as(crate::me(), shell, aliases, name)
}

/// [`init`] for the program installed as `bin` (`kiwa` or `kw`).
pub fn init_as(bin: &str, shell: Shell, aliases: &[Alias], name: Option<&str>) -> String {
    let mut out = base(bin, shell, aliases);
    if let Some(n) = name.filter(|n| valid_name(n) && *n != bin) {
        out.push_str(&format!("# the picker as `{n}`\n"));
        out.push_str(&picker_command(bin, shell, n));
        if matches!(
            shell,
            Shell::Bash | Shell::Zsh | Shell::Fish | Shell::Elvish
        ) {
            out.push_str(&completions_for(shell, n));
        }
    }
    out
}

fn base(bin: &str, shell: Shell, aliases: &[Alias]) -> String {
    let mut out = String::new();
    let aliases: Vec<&Alias> = aliases.iter().filter(|a| valid_name(&a.name)).collect();
    match shell {
        Shell::Bash | Shell::Zsh => {
            out.push_str("# kiwa shell integration\n");
            out.push_str(&format!("kcd() {{\n  local dir\n  dir=\"$(command {bin} path \"$@\")\" && builtin cd -- \"$dir\"\n}}\n"));
            for a in aliases {
                out.push_str(&format!(
                    "{}() {{ command {bin} {} \"$@\"; }}\n",
                    a.name,
                    q(&a.name)
                ));
            }
            out.push_str(&completions_for(shell, bin));
        }
        Shell::Fish => {
            out.push_str("# kiwa shell integration\n");
            out.push_str(&format!("function kcd\n    set -l dir (command {bin} path $argv); and builtin cd -- $dir\nend\n"));
            for a in aliases {
                out.push_str(&format!(
                    "function {}\n    command {bin} {} $argv\nend\n",
                    a.name,
                    q(&a.name)
                ));
            }
            out.push_str(&completions_for(shell, bin));
        }
        Shell::Powershell => {
            out.push_str("# kiwa shell integration\n");
            out.push_str(&format!("function kcd {{\n    $dir = & {bin} path @args\n    if ($LASTEXITCODE -eq 0) {{ Set-Location -LiteralPath $dir }}\n}}\n"));
            for a in aliases {
                out.push_str(&format!(
                    "function {} {{ & {bin} {} @args }}\n",
                    a.name,
                    ps_q(&a.name)
                ));
            }
            out.push_str(&completions_for(shell, bin));
        }
        Shell::Elvish => {
            out.push_str("# kiwa shell integration\n");
            out.push_str(&format!("fn kcd {{|@a| cd ({bin} path $@a) }}\n"));
            for a in aliases {
                out.push_str(&format!(
                    "fn {} {{|@a| {bin} {} $@a }}\n",
                    a.name,
                    q(&a.name)
                ));
            }
            out.push_str(&completions_for(shell, bin));
        }
        Shell::Nushell => {
            out.push_str("# kiwa shell integration\n");
            out.push_str(&format!(
                "def --env kcd [...args] {{ cd (^{bin} path ...$args | str trim) }}\n"
            ));
            for a in aliases {
                out.push_str(&format!(
                    "def {} [...args] {{ ^{bin} {} ...$args }}\n",
                    a.name, a.name
                ));
            }
            out.push_str(&completions_for(shell, bin));
        }
    }
    out
}

/// The one line to add to the shell profile, with `--name` when one was given.
pub fn profile_line(shell: Shell, name: Option<&str>) -> String {
    profile_line_as(crate::me(), shell, name)
}

fn profile_line_as(bin: &str, shell: Shell, name: Option<&str>) -> String {
    // Each template holds `kw shell-init <shell>` once; it becomes the program's own name.
    let word = shell_word(shell);
    let flag = name.map(|n| format!(" --name {n}")).unwrap_or_default();
    default_profile_line(shell).replacen(
        &format!("kw shell-init {word}"),
        &format!("{bin} shell-init {word}{flag}"),
        1,
    )
}

fn shell_word(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Fish => "fish",
        Shell::Powershell => "powershell",
        Shell::Elvish => "elvish",
        Shell::Nushell => "nushell",
    }
}

fn default_profile_line(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => "eval \"$(kw shell-init bash)\"            # ~/.bashrc",
        Shell::Zsh => "eval \"$(kw shell-init zsh)\"             # ~/.zshrc (after compinit)",
        Shell::Fish => "kw shell-init fish | source              # ~/.config/fish/config.fish",
        Shell::Powershell => {
            "Invoke-Expression (& { (kw shell-init powershell | Out-String) })   # $PROFILE"
        }
        Shell::Elvish => "eval (kw shell-init elvish | slurp)      # ~/.config/elvish/rc.elv",
        Shell::Nushell => "kw shell-init nushell | save -f ~/.kiwa.nu   # then: source ~/.kiwa.nu",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(name: &str) -> Alias {
        Alias {
            name: name.into(),
            project: "api".into(),
            task: "test".into(),
            args: vec![],
        }
    }

    #[test]
    fn every_shell_defines_kcd_and_aliases() {
        for sh in [
            Shell::Bash,
            Shell::Zsh,
            Shell::Fish,
            Shell::Powershell,
            Shell::Elvish,
            Shell::Nushell,
        ] {
            let s = init_as("kw", sh, &[alias("t")], None);
            assert!(s.contains("kcd"), "{sh:?}");
            assert!(s.contains("kw path"), "{sh:?}");
            let k = init_as("kiwa", sh, &[alias("t")], None);
            assert!(
                k.contains("kiwa path") && !k.contains("kw path"),
                "{sh:?}: the installed name is used"
            );
            assert!(
                s.contains(" t") || s.contains("\nt(") || s.starts_with("t("),
                "{sh:?}"
            );
            assert!(s.len() > 300, "{sh:?} completions missing");
        }
    }

    #[test]
    fn unsafe_alias_names_are_skipped() {
        let s = init(Shell::Bash, &[alias("rm -rf /;x")], None);
        assert!(!s.contains("rm -rf"));
        let s = init(Shell::Bash, &[], Some("a;b"));
        assert!(!s.contains("a;b"), "an invalid name is never written");
    }

    #[test]
    fn the_picker_under_a_name_in_every_shell() {
        let cases = [
            (Shell::Bash, "kp() { command kw \"$@\"; }", true),
            (Shell::Zsh, "kp() { command kw \"$@\"; }", true),
            (Shell::Fish, "function kp\n    command kw $argv\nend", true),
            (Shell::Powershell, "function kp { & kw @args }", false),
            (Shell::Elvish, "fn kp {|@a| kw $@a }", true),
            (Shell::Nushell, "def kp [...args] { ^kw ...$args }", false),
        ];
        for (sh, def, completes) in cases {
            let s = init_as("kw", sh, &[], Some("kp"));
            assert!(s.contains(def), "{sh:?}: {s}");
            let named = s.len() - init_as("kw", sh, &[], None).len();
            assert_eq!(named > 300, completes, "{sh:?}: completions for kp");
            assert!(
                profile_line_as("kiwa", sh, Some("kp"))
                    .contains(&format!("kiwa shell-init {} --name kp", shell_word(sh))),
                "{sh:?}"
            );
        }
        assert!(!profile_line_as("kw", Shell::Bash, None).contains("--name"));
        assert!(profile_line_as("kw", Shell::Fish, None).starts_with("kw shell-init fish"));
        // `kiwa shell-init bash --name kw`: the short name as a function.
        let s = init_as("kiwa", Shell::Bash, &[], Some("kw"));
        assert!(s.contains("kw() { command kiwa \"$@\"; }"), "{s}");
        assert!(
            !init_as("kw", Shell::Bash, &[], Some("kw")).contains("kw() { command"),
            "never a function calling itself"
        );
    }

    #[test]
    fn names_that_would_shadow_a_command_are_refused() {
        let empty = Some(std::ffi::OsString::new());
        let why = |n: &str| shadowing(n, &[alias("t")], empty.clone());
        assert!(why("cd").unwrap().contains("builtin"));
        assert!(why("ls").is_some());
        assert!(why("kiwa").unwrap().contains("kiwa"));
        assert_eq!(why("kw"), None, "`kw` is free when nothing on PATH has it");
        assert!(why("t").unwrap().contains("alias"));
        assert!(why("a b").unwrap().contains("letters"));
        assert_eq!(why("kp"), None);
        // A program on PATH.
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(if cfg!(windows) {
            "mytool.exe"
        } else {
            "mytool"
        });
        std::fs::write(&exe, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = Some(dir.path().as_os_str().to_owned());
        let r = shadowing("mytool", &[], path).unwrap();
        assert!(r.contains("would hide") && r.contains("mytool"), "{r}");
    }

    #[test]
    fn quoting() {
        assert_eq!(q("it's"), "'it'\\''s'");
        assert_eq!(ps_q("it's"), "'it''s'");
    }
}
