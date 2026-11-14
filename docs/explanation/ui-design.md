# Interface design rules

The picker must be readable in a second, in any terminal theme, with or without colour.
These rules are enforced in `crates/kiwa/src/theme.rs` and covered by rendering tests in
`crates/kiwa/src/tui.rs` (`picker_tests`).

| Rule | Why |
|---|---|
| **The accent marks the cursor position, and only that**: the selected row, the active scope, the prompt. Borders take a dimmed variant. | A single focal colour tells the eye where to look; a second accent competes with it. |
| **Green, yellow and red are result colours.** None of them can be picked as the accent. | If the accent were green, a selected row and a successful run would look the same. Status colours keep one meaning. |
| **Never colour alone.** Every coloured state also has a symbol or a word: `✔` / `✘` / `■` / `⚠` / `ℹ`, `●` for the selection, `○` for hover; a focused button is reversed. | [WCAG 2.2 SC 1.4.1 Use of Color](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html); about 1 in 12 men has a colour-vision deficiency. |
| **Hierarchy from bold and the terminal's faint attribute** (SGR 2), not a fixed grey. | Faint is rendered by the terminal relative to its own foreground, so it stays legible on light and dark themes, where a fixed grey can vanish. |
| **`NO_COLOR` removes colours, keeps bold/faint/underline.** | [no-color.org](https://no-color.org) asks programs not to add ANSI *colour*; attributes are not colours and keep the structure visible. |
| **Matched characters are underlined**, not recoloured. | Underline works with every accent and without colour, and does not fight the selection colour. |
| **Rows of the same project show its name faint after the first.** | Groups read at a glance, yet every row stays complete (search, copy, screen readers). |
| **Monorepo packages sit under their root while browsing** (indented, path below the root, `▾`/`▸` to fold), **and the list is flat again as soon as you search.** A package is what its root declares (`workspaces`, `pnpm-workspace.yaml`, Cargo `[workspace]`, `go.work`, uv, Gradle settings, `.sln`), exclusions included; a project nested in a folder that declares nothing falls back to folder nesting, and the details say which file decided (`monorepo shop · pnpm-workspace.yaml`). A workspace without nested projects looks exactly as before. | Browsing answers "what is in this repository?"; searching answers "where is `test`?", and a fold must never hide a match. |
| **`→` unfolds only on a folded group**, elsewhere it keeps asking for arguments; `←` folds the group of the selected row. The footer says which. | The usual tree convention, without taking `→` away from the most frequent action. |
| **A branch column when the terminal is wide enough**, on a project's first row, read from `.git/HEAD` without running git. **The default branch is faint** (the remote's from `origin/HEAD`, otherwise `main`, `master`, `trunk` or `develop`), so a feature branch stands out; the column never takes the room of the last-run tag. | "Am I on the right branch?" is answered before running, at no cost: two file reads per repository. |
| **Every key is on screen.** The footer lists the keys of the current mode; nothing is hidden. | Discoverability for first-time users; no manual needed. |
| **The footer is also the toolbar.** Its entries that act on the list (`⏎ run`, `→ args`, `esc …`) are clickable. | Every action stays one click away, including in a terminal too narrow for the details pane, without drawing buttons over the rows. |
| **One line of chrome at the top.** A badge in the accent, the folders, the project in scope as a path (`~/dev / billing`), the counts, then a rule. | The list gets the height; the header reads in one glance. |
| **The divider is the scrollbar.** The column between the list and the details shows a thumb when the list overflows; clicking the track pages. | Position and length of the list at a glance, with no rows spent on scroll hints. |
| **Results stay on the normal screen.** The picker uses the alternate screen; task output and the closing rule `── ✔ api test · ok · 2.7s ───` are printed on the normal screen, so they remain in the scrollback. | You can scroll back to an error after returning to the picker. |
| **Last result per task in the list** (`✔ 2 min ago`) and in the details (`last run ✔ exit 0 · 2 min ago · 5 runs`). | Answers "did this pass last time?" before running it again. |
| **Single-width characters for structure** (`─│┃●○❯▾▸`). | Rules and markers stay aligned in every terminal and font. |
| **Explicit empty states** ("No projects found here: run `kw init`", "No match for “x”: esc clears"). | An empty screen without a next step is a dead end. |
| **The mouse is optional** and switched off where it cannot work (mintty). | Keyboard users lose nothing; mouse users get hover, click and wheel. |

## Accents

`cyan` (default), `azure`, `indigo`, `violet`, `rose`, `amber`, `steel`, and `none`, which
keeps the accent bold but uncoloured. Apart from cyan they are xterm-256 indexes, picked for
contrast on both light and dark backgrounds.

## Snapshot

`cargo test -p kiwa --lib -- --ignored --nocapture snapshot` renders the picker on
`examples/demo` as text. Use it in reviews of any visual change.
