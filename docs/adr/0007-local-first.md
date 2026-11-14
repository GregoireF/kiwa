# ADR 0007: Local-first: no network call, no update check

Status: accepted (2026-11-08)

## Context
A common CLI pattern is a daily background request asking whether a newer release exists.
It is convenient, but it is a network call made on the user's behalf, it reveals usage, it
fails or slows down offline and behind proxies, and it must be disabled in CI.

## Decision
kiwa makes no network call, ever: no update check, no telemetry. Updates are announced by
the channel the user installed from (`brew upgrade`, `scoop update`, the shell installer
run again), and release notes are on GitHub.

## Consequences
+ Works the same offline, behind a proxy, in CI and in air-gapped environments.
+ Nothing to disclose in a privacy notice; nothing to configure.
- Users who installed by hand learn about new versions later.
