# Fuzzing

Fuzz targets for the manifest parsers that read user files: Makefile, justfile, Taskfile,
Rakefile and the KDL config. Each target feeds arbitrary text to a parser, which must return
without panicking or hanging.

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run make -- -max_total_time=60     # or just, taskfile, rake, config
```

CI runs every target for 10 minutes each week (`.github/workflows/fuzz.yml`) and opens one
issue per failing target with the crashing input attached.
