# Release kiwa (maintainers)

Releases are built by [dist](https://github.com/axodotdev/cargo-dist) from a `v*` tag
(`.github/workflows/release.yml` is generated: change `dist-workspace.toml`, then run
`cargo xtask dist-generate`).

## Checklist

1. `cargo xtask check` and `cargo xtask bench` are green; `CHANGELOG.md` has the version's section.
2. Set the version in the root `Cargo.toml` (`[workspace.package] version`).
3. For a release candidate, tag `v0.1.0-rc.1`: GitHub marks it as a pre-release and the
   Homebrew/Scoop tap is **not** updated. Keep candidates for two weeks before the final tag.
4. Tag (signed) and push: `git tag -s v0.1.0 -m v0.1.0 && git push origin v0.1.0`. dist
   builds 7 targets, the shell and PowerShell installers, a CycloneDX SBOM and build
   provenance, then calls the publish jobs: `publish-crates.yml` publishes `kiwa-core` then
   `kiwa` to crates.io, and `publish-tap.yml` writes `Formula/kiwa.rb` and `bucket/kiwa.json`
   into `GregoireF/homebrew-tap`. Each waits for an approval of the `release` environment.
5. Verify an archive: `gh attestation verify kiwa-x86_64-unknown-linux-gnu.tar.xz -R GregoireF/kiwa`.

## One-time setup

- Environment `release` with a required reviewer: the human gate before publishing.
- A deploy key for the tap, as for ship-it (the job token only reaches this repository):
  generate an ed25519 key pair, add the public half to `GregoireF/homebrew-tap` as a deploy
  key with write access (`gh repo deploy-key add … --allow-write`), store the private half as
  the environment secret `HOMEBREW_TAP_SSH_KEY` of `release` (`gh secret set … --env release`),
  then delete both files from your disk.
- A crates.io API token scoped to `publish-new` and `publish-update` for `kiwa` and
  `kiwa-core`, stored as the environment secret `CARGO_REGISTRY_TOKEN` of `release`
  (`gh secret set CARGO_REGISTRY_TOKEN --env release -R GregoireF/kiwa`, pasted at the prompt).
- Repository variable `CODE_SCANNING=true` once code scanning is available (public repository).
- Ruleset on `main`: pull request required, required checks `ci passed`, `title`, `zizmor`,
  `actionlint`, `signoff`.
