# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| latest  | :white_check_mark: |
| < latest| :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability, **please do NOT open a public issue.**

Instead, report it privately via
[GitHub Security Advisories](https://github.com/Xuepoo/agent-lx-music/security/advisories/new).

### What to include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

### Response timeline

- **Acknowledgment**: within 48 hours
- **Initial assessment**: within 1 week
- **Fix & release**: depends on severity, but we aim for critical issues within 2 weeks

## Dependency Security

We use:

- **cargo-deny** in CI to audit dependencies for known vulnerabilities
- **Dependabot** for automated dependency update PRs
- **Docker Scout** (via Docker Hub) for container image scanning

## Supply Chain Security

Release artifacts are built exclusively in GitHub Actions. Binary checksums
(SHA256) are published alongside each release for verification.
