# ADR 0008: Running a task takes a deliberate action

Status: accepted (2026-11-08)

## Context
Tasks include `deploy`, `release` and `db:reset`. A launcher must make browsing cheap and
running deliberate, so that no stray key or click starts one.

## Decision
- Fuzzy matching only *suggests* in direct mode; it never picks what runs.
- In the picker, a click selects and a second click on the selected row runs it. When the
  picker opens, clicks only count once the pointer has moved or a key was pressed, so the
  press that opened it (or the second half of a double-click) cannot run anything; terminals
  that never report motion fall back to a 600 ms delay.
- Right-click selects a row and asks for its arguments; it never runs a task.
- After a run, Enter goes **back** to the picker; running again takes `r` or the button.
  A stray Enter must never start a deploy twice.
- The footer's `⏎ run` acts on the selected row, never on the row under the pointer.

## Consequences
+ No task runs by accident from the keyboard or the mouse.
- One more key than "Enter runs again" for people who repeat a task often (`r`).
