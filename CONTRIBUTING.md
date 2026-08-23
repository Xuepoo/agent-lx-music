# Contributing to agent-lx-music

Thank you for your interest in contributing! This document outlines the
development workflow and quality standards for this project.

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable, edition 2024)
- [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) (dependency auditing)
- [lefthook](https://lefthook.dev/) (local Git hooks)
- [markdownlint-cli2](https://github.com/DavidAnson/markdownlint-cli2) (Markdown linting)

## Git Hooks

Hooks are managed by [lefthook](https://lefthook.dev/) via `.lefthook.yml`.
After cloning, activate them once:

```bash
lefthook install
```

What runs when:

| Hook         | Trigger            | Commands                                                                               |
| ------------ | ------------------ | -------------------------------------------------------------------------------------- |
| `pre-commit` | staged `.rs` files | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-features -- -D warnings` |
| `pre-commit` | staged `.md` files | `markdownlint-cli2 <staged files>`                                                     |
| `pre-push`   | every push         | `cargo check`, `cargo test --workspace --all-features`                                 |

The pre-commit hooks run in parallel. To run them manually against all
files without committing:

```bash
lefthook run pre-commit --all-files
```

## Development Setup

```bash
git clone https://github.com/Xuepoo/agent-lx-music.git
cd agent-lx-music
cargo build
cargo test
```

## Code Quality

Before submitting a PR, ensure:

```bash
# Formatting
cargo fmt --check

# Linting
cargo clippy -- -D warnings

# Tests
cargo test

# Dependency audit (if deny.toml exists)
cargo deny check
```

## Commit Convention

We use [Conventional Commits](https://www.conventionalcommits.org/):

| Prefix      | Usage                        |
| ----------- | ---------------------------- |
| `feat:`     | New feature                  |
| `fix:`      | Bug fix                      |
| `docs:`     | Documentation only           |
| `refactor:` | Code change (no feature/fix) |
| `test:`     | Adding or updating tests     |
| `ci:`       | CI/CD changes                |
| `deps:`     | Dependency updates           |
| `chore:`    | Maintenance tasks            |

## Pull Request Process

1. Fork the repo and create a feature branch from `main`
2. Make your changes with clear, atomic commits
3. Ensure all CI checks pass
4. Open a PR using the provided template
5. Wait for review — we aim to respond within 48 hours

## Release Process

Releases are automated via CI. When a version tag (`v*`) is pushed:

1. CI builds binaries for all supported platforms
2. Packages (.deb, .rpm, .pkg.tar.zst) are created
3. GitHub Release is published
4. Crates.io, Docker Hub, AUR, Homebrew, and Scoop are updated

To create a release:

```bash
# Bump version in Cargo.toml
cargo release patch  # or minor, major
git tag v0.x.y
git push origin v0.x.y
```

## License

By contributing, you agree that your contributions will be licensed
under the [MIT License](LICENSE).
