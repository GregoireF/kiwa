# ADR 0002: KDL v2 for configuration

Status: accepted (2026-11-06)

## Options
- TOML: Rust standard, verbose for lists of nested items.
- YAML: familiar to devops, indentation and implicit typing pitfalls (the "Norway problem").
- JSON: no comments.
- KDL v2: node-based, comments, readable, typed values; growing adoption (Zellij, niri).

## Decision
KDL v2 via the `kdl` crate (v6). Zero config remains the default.

## Consequences
+ Distinctive and pleasant; maps naturally to future constructs (`project "x" { tag "work" }`,
  user-defined detectors).
- Less known: mitigated with documented examples and clear parse errors with locations.
- Unknown nodes warn instead of failing, for forward compatibility.
