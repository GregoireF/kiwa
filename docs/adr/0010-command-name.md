# ADR 0010: The command is `kiwa`, with `kw` where it is free

Status: accepted (2026-11-13). Supersedes the command name in
[ADR 0005](0005-name-license-distribution.md).

## Context
The command was `kw`. Debian and Ubuntu package `kworkflow`, a Linux kernel developer tool that
installs `/usr/bin/kw`. A distribution package of kiwa installing `kw` would conflict with it, and
a user with both would get one or the other depending on `PATH`.

## Decision
- The program is `kiwa`. `kw` is the same program under a second binary name.
- The shell and PowerShell installers, Homebrew, Scoop, winget, npm and PyPI install both names;
  `.deb`, `.rpm` and AUR packages install `kiwa` only.
- `kiwa shell-init <shell> --name kw` adds the short name as a shell function where nothing else
  is called `kw`; it refuses otherwise and says what it would hide (K-36).
- Help, errors, suggestions and generated shell code use the name that was typed (`crate::me()`),
  so `kiwa run …` and `kw run …` both tell the truth.
- The documentation writes `kw` and says so once.

## Consequences
+ kiwa installs everywhere, next to kworkflow.
+ The short name stays for most users, who never meet kworkflow.
- Two names to explain; examples copied from the docs need `kiwa` on a distribution package
  unless `--name kw` was added.
