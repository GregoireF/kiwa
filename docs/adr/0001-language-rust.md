# ADR 0001: Rust

Status: accepted (2026-11-06)

## Context
A CLI launched dozens of times a day must start instantly, ship as one file on Windows,
macOS and Linux, and attract contributors.

## Options
| | Startup | Distribution | TUI ecosystem | Contributors | Notes |
|---|---|---|---|---|---|
| Rust | ~1 ms | static binary, cargo-dist | ratatui, crossterm, nucleo | large OSS CLI community (ripgrep, fd, zoxide, uv, mise) | steeper learning curve |
| Go | ~2 ms | trivial cross-compile | Bubble Tea | easy onboarding | GC, larger binaries |
| TypeScript/Bun/Node | 30–150 ms | needs runtime or big bundle | ink | largest pool | start-up cost on every invocation |
| Zig | ~1 ms | excellent | immature | small | ecosystem too young |

## Decision
Rust, edition 2024, MSRV 1.85.

## Consequences
+ Fastest startup, small static binary, memory safety, `unsafe` forbidden workspace-wide.
+ The most-starred modern CLIs are Rust: users trust and recognise the stack.
- Compile times; mitigated with a small dependency budget.
