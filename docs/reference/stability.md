# Stability

Until 1.0, anything may change, but every change to the items below is listed in the
[changelog](../../CHANGELOG.md). From 1.0, they follow semantic versioning: a change that
breaks them needs a major version, after a deprecation period.

## Covered

- **Commands and flags**: every command and flag of the [reference](cli.md), their meaning and defaults.
- **Environment variables**: `KIWA_CONFIG`, `KIWA_STATE`, `KIWA_HOME`, `KIWA_JSON`,
  `KIWA_WORKSPACE`, and the honoured `NO_COLOR`, `FORCE_COLOR`, `VISUAL`, `EDITOR`.
- **Config keys and location**: `version`, `roots`, `depth`, `ignore`, `accent`, `mouse`,
  `hide`, `risky`, `show-git-only`, `alias`, `workspace` (with `roots`, `depth`, `ignore`), and the
  platform paths of [configuration](configuration.md). A format change comes with a new
  `version`, an automatic migration and a `.bak` copy.
- **Exit codes**: 0, 1, 2, 3, 4, 5, 127, 130, and the task's own code in direct mode
  ([table](json-and-exit-codes.md#exit-codes)).
- **JSON**: the `kiwa/v1` envelope and every `kind`. Fields may be **added** within `kiwa/v1`;
  removing or renaming one requires `kiwa/v2`.

## Not covered

- The picker's layout, colours and key hints (the documented keys stay; the looks may improve).
- Human-readable text output: parse `--json` instead.
- Which tasks a detector finds for a given manifest, when a detector becomes more accurate.
