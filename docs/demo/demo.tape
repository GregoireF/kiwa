# The README demo, recorded with VHS (https://github.com/charmbracelet/vhs) in Docker:
# `cargo xtask demo` builds docs/demo/Dockerfile (kiwa built from this checkout on top of the
# VHS image) and runs this file from the repository's root, mounted at /vhs. The config and the
# history live in /tmp of the container: nothing on the host is read or written but the GIF.
# Edit this file, then run `cargo xtask demo` again.

Output docs/demo/demo.gif

Set Shell "bash"
Set FontSize 15
Set Width 1280
Set Height 720
Set Padding 18
Set TypingSpeed 70ms
Set Theme "Catppuccin Mocha"

Hide
Type `export PS1='$ ' KIWA_CONFIG=/tmp/kiwa.kdl KIWA_STATE=/tmp/kiwa-state.json`
Enter
Type `printf 'version 1\nroots "/vhs/examples/demo"\nworkspace "shop" {\n    roots "/vhs/examples/demo/shop"\n}\n' > /tmp/kiwa.kdl`
Enter
Type "cd examples/demo && clear"
Enter
Sleep 300ms
Show

# Every project and task of a polyglot folder, with no setup.
Type "kw"
Sleep 400ms
Enter
Sleep 2.5s

# One fuzzy search over projects and tasks, then run.
Type "hel gr"
Sleep 1.2s
Enter
Sleep 2s

# Back to the picker: the task is in `recent`, with its result.
Enter
Sleep 2s

# Workspaces behind `Folders`.
Tab
Sleep 500ms
Enter
Sleep 1.5s
Down
Sleep 500ms
Enter
Sleep 2s

# `@name` narrows the search to a workspace without leaving the picker.
Type "@shop dev"
Sleep 1.5s
Ctrl+U
Sleep 400ms
Escape
Sleep 600ms

# What a project is, and how to get it running.
Type "kw about shop"
Sleep 400ms
Enter
Sleep 3s
Type "kw setup shop -n"
Sleep 400ms
Enter
Sleep 3s

# Agents get the same answers as data: the exact command, without running it.
Type "kw run hello greet -n --json"
Sleep 400ms
Enter
Sleep 3s
