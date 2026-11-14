# Use kw from an AI agent

kw gives agents an inventory of everything runnable and a safe way to run it.

1. **Install the skill** for Claude Code, Codex, Cursor or Copilot: copy
   [`skills/kiwa/SKILL.md`](../../skills/kiwa/SKILL.md) into the agent's skills folder
   (`kw agent skill` will do it in 0.2).
2. **Always use `--json`**: one `kiwa/v1` envelope on stdout; task output goes to stderr.
3. **Discover, then dry-run, then run**:

   ```sh
   kw list --json                       # projects, tasks, exact argv, runner and why
   kw run api test -n --json            # what would run, in which folder
   kw run api test --json -- -v         # run it; stdout ends with {"kind":"run","data":{"exit_code":0,…}}
   ```

4. **Handle errors from data**: `error.kind` (`not-found`, `ambiguous`, `runner-missing`, …)
   and `error.suggestions`. Never retry with a guessed name; fuzzy matching only suggests.

Exit codes and shapes: [JSON and exit codes](../reference/json-and-exit-codes.md).
