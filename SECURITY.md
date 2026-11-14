# Security policy

## Supported versions

Before 1.0, only the latest release receives fixes.

## Reporting a vulnerability

Please **do not open a public issue**. Use GitHub's private vulnerability reporting
("Security" tab → "Report a vulnerability"). You will get an acknowledgement within
5 working days and a fix or mitigation plan within 30 days for confirmed issues.
We coordinate disclosure with you and credit you unless you prefer otherwise.

## Security model

- Discovery reads files only; it never executes project code.
- Tasks run as argv without a shell, in the project directory, with your user's permissions.
  Running a task from an untrusted repository is as dangerous as running it by hand.
- kiwa makes no network calls.

Releases will ship with an SBOM (CycloneDX) and build provenance attestations, in line
with the EU Cyber Resilience Act expectations for open-source components.
