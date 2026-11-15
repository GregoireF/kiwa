# Security policy

## Supported versions

Before 1.0, only the [latest release](https://github.com/GregoireF/kiwa/releases/latest)
receives fixes.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through
[GitHub's private vulnerability reporting](https://github.com/GregoireF/kiwa/security/advisories/new)
(the "Security" tab, then "Report a vulnerability"). You will get an acknowledgement within
5 working days and a fix or mitigation plan within 30 days for confirmed issues. We coordinate
disclosure with you, publish a [security advisory](https://github.com/GregoireF/kiwa/security/advisories)
once a fix is out, and credit you unless you prefer otherwise.

## Security model

- **Discovery reads files only**: it never executes project code
  ([ADR 0003](docs/adr/0003-static-discovery.md)).
- **Tasks run as an argv**, in the project folder, with your user's permissions. A line that
  needs a shell (from a README, a CI job, a Procfile…) runs through `sh -c` only after it is
  shown and confirmed, and so do tasks whose name looks risky (`deploy`, `drop`, `…:prod`)
  ([ADR 0009](docs/adr/0009-shell-lines-shown-and-confirmed.md)). Without a terminal, they need
  `--yes`. Running a task from an untrusted repository is as dangerous as running it by hand.
- **No network calls** ([ADR 0007](docs/adr/0007-local-first.md)): no telemetry, no update
  check. The only outward action is opening a dev server's local URL in your browser when you
  press Ctrl+O.
- **Agents** get the same confirmations: a `shell`, `risky` or `suggested` task fails with
  `confirmation-required` until `--yes` is passed
  ([JSON contract](docs/reference/json-and-exit-codes.md)).

## Supply chain

Each release ships a CycloneDX SBOM and build provenance attestations
(`gh attestation verify <file> --repo GregoireF/kiwa`). Dependencies are reviewed by
Dependabot with a 7-day cooldown, workflows are checked by
[zizmor](https://docs.zizmor.sh) and actionlint, and the project is scored by
[OpenSSF Scorecard](https://scorecard.dev/viewer/?uri=github.com/GregoireF/kiwa).
