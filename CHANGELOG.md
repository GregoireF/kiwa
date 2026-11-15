# Changelog

All notable changes are documented here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
versioning: [SemVer](https://semver.org/).

## [Unreleased]

### Changed
- Minimum supported Rust version 1.88 (was 1.85): the `ignore` crate needs it since 0.4.30.

### Security
- ratatui 0.30: drops `lru` 0.12 (RUSTSEC-2026-0002, RUSTSEC-2026-0253) and the unmaintained
  `paste` (RUSTSEC-2024-0436).

### Fixed
- Linux: stopping kw (SIGTERM, SIGHUP, SIGQUIT) also stops the processes its task started, such
  as the `node` behind `npm run`, which `dash` leaves running when only `npm` is signalled.

[Unreleased]: https://github.com/GregoireF/kiwa/commits/main
