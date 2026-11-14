# Architecture Decision Records

Each record states the context, the options, the decision and its consequences. A decision
is changed by a new record that supersedes the old one, never by editing it silently.

| ADR | Decision |
|---|---|
| [0001](0001-language-rust.md) | Rust, edition 2024, MSRV 1.85 |
| [0002](0002-config-kdl.md) | KDL v2 configuration, edited in place |
| [0003](0003-static-discovery.md) | Static discovery, no shell |
| [0004](0004-agent-contract.md) | Agent-native contract: JSON, skill, MCP |
| [0005](0005-name-license-distribution.md) | Name, license, distribution |
| [0006](0006-measured-runner-behaviour.md) | Runner behaviour is measured, not assumed |
| [0007](0007-local-first.md) | Local-first: no network call, no update check |
| [0008](0008-deliberate-runs.md) | Running a task takes a deliberate action |
| [0009](0009-shell-lines-shown-and-confirmed.md) | A shell line runs only once shown and confirmed (amends 0003) |
| [0010](0010-command-name.md) | The command is `kiwa`, with `kw` where it is free (supersedes 0005's name) |
