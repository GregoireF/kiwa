# Terminal compatibility

The picker uses [ratatui](https://ratatui.rs) on [crossterm](https://github.com/crossterm-rs/crossterm).
Everything except the picker (`list`, `run`, `--json`, ...) is plain text and works anywhere.

| Terminal | Picker | Mouse | Status |
|---|---|---|---|
| Windows Terminal | ✔ | ✔ | verified (Windows 11, 2026-11-07) |
| VS Code integrated terminal | ✔ | ✔ | to verify |
| Windows console host (conhost) | ✔ | ✔ | to verify |
| Git Bash / mintty | ⚠ | off automatically | crossterm needs the Windows console API, which mintty's pty does not expose ([crossterm#580](https://github.com/crossterm-rs/crossterm/issues/580)). Run `winpty kw` or use Windows Terminal. |
| macOS Terminal, iTerm2 | ✔ | ✔ | to verify |
| Linux (GNOME Terminal, Konsole, kitty, Alacritty, WezTerm) | ✔ | ✔ | to verify |
| tmux / screen | ✔ | needs `set -g mouse on` | to verify |

"To verify" rows are expected to work from crossterm's documented support and must be
checked by hand before 1.0. Reports welcome.

## Measuring a terminal

`kw doctor` prints what kiwa sees without asking anything: the terminal it guesses from the
environment (`TERM_PROGRAM`, `WT_SESSION`, `KITTY_WINDOW_ID`, `KONSOLE_VERSION`,
`GNOME_TERMINAL_SCREEN`, `VTE_VERSION`, `MSYSTEM`), the colour depth it advertises (`COLORTERM`,
`TERM`), whether the mouse is on and why not, the config and state files, and which of the 26
runners are on `PATH`. It reads variables and files only; it starts no process.

`kw doctor --probe` opens a screen that lists what to try (left, right and middle click, motion
without a button, drag, wheel up and down, resizing) and ticks each one when the terminal reports
it. A 24-bit gradient is drawn too: press `t` if it is smooth (no bands). `q`, Esc or Enter ends
the probe and prints a row for the table below; `--json` prints the result as `doctor-probe`.

| Terminal and version | OS | Clicks | Motion, drag | Wheel | Resize | Truecolor |
|---|---|---|---|---|---|---|

Rows are added only from a probe run, with the terminal's version. Until ten terminals are listed
(Windows Terminal, conhost, VS Code, mintty, macOS Terminal, iTerm2, GNOME Terminal, Konsole,
Kitty, WezTerm), the table above stays the summary.

## When the picker cannot start

If the terminal cannot enter raw mode, `kw` prints an error suggesting `kw list` or
`kw <project> <task>` instead of crashing. Without a TTY (pipes, CI, agents) `kw` never
opens the picker: it lists projects, or runs the task you named.

## Accessibility

- `NO_COLOR=1` removes every colour; meaning is never carried by colour alone
  (✔/✘ symbols, text labels, reversed selection).
- Everything is reachable from the keyboard; every key is shown in the footer.
- `--no-mouse` or `mouse #false` in the config disables mouse capture, which also
  restores the terminal's native text selection.
