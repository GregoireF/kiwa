# ADR 0003: Static discovery, no shell

Status: accepted (2026-11-06); the "no shell" part is amended by
[ADR 0009](0009-shell-lines-shown-and-confirmed.md): a line that needs a shell runs only after it
is shown and confirmed.

## Context
Listing tasks could call `just --summary`, `npm run`, `task --list-all`, `gradle tasks`.
That is accurate but slow (process spawns, JVM boot), requires every tool installed, and
**executes code from repositories that may be untrusted** (Gradle scripts, Rakefiles).

## Decision
Detectors only read files. Tasks are executed as argv without a shell.

## Consequences
+ Scanning 30+ projects in tens of ms; safe on any clone; works with no runner installed.
+ Identical behaviour across OSes; no quoting or injection issues.
- Dynamic tasks (generated Makefile targets, Gradle custom tasks) can be missed.
  Mitigation: common conventions covered, opt-in "deep" mode later, user KDL detectors.
- Minimal parsers (Makefile, justfile, Taskfile, Rakefile) have unit tests per edge case.
