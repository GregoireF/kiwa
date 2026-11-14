# ADR 0005: Name, license, distribution

Status: accepted (2026-11-06)

## Name
Project `kiwa`, binary `kw` (the ripgrep/`rg` pattern). On 2026-11-06, `kiwa` was free on crates.io,
npm and PyPI. All 2–3 letter candidates checked (wsx, wrk, wo, dax, hop, nav...) were taken
on npm. `kw` is typed with one hand. Still to check: GitHub org, domain, EUIPO trademark search.

## License
`MIT OR Apache-2.0`: Rust ecosystem norm, maximum adoption, Apache patent grant.
EUPL-1.2 considered (EU signal) but rejected: friction for companies. REUSE-compliant.

## Distribution
v0.1: GitHub Releases via cargo-dist (Windows/macOS/Linux, x64 + arm64), shell and PowerShell
installers, SBOM + build provenance attestations; crates.io + cargo-binstall; npm and PyPI
wrapper packages (`npx kiwa`, `uvx kiwa`) that fetch the matching binary.
v0.2: Homebrew, Scoop, winget, AUR, Nix.
