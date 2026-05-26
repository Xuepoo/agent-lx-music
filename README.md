# agent-lx-music

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [日本語](README.ja.md) | [Français](README.fr.md) | [Español](README.es.md)

A Unix-philosophy CLI music player powered by lx-music sources, written in Rust. It drops Electron entirely, running JS scrapers inside an isolated QuickJS sandbox and delegating high-fidelity audio playback to a headless `mpv` instance over a detached POSIX daemon loop.

---

## Key Features

- **QuickJS Sandbox Integration**: Executes standard `lx-music` javascript sources securely inside a highly optimized [rquickjs](https://github.com/DelSkayn/rquickjs) sandbox environment.
- **Decoupled POSIX Daemonization**: Spawns headless `mpv` playback loops inside detached `setsid` process groups, letting you manage and check playback states asynchronously without locking terminals.
- **SQLite Transparent Caching**: Provisions a local database to store playlists, playback histories (with auto-purges), favorites, and transparently cache LRC lyrics for zero-network lookups.
- **Static LRC Lyrics & Cover Downloads**: High-speed, high-reliability extraction of synchronized lyrics (with translation and romanized fallbacks) and Magic-Bytes based image suffix detection.
- **Audio-Enabled Containerization**: Fully compatible with rootless Podman/Docker, enabling isolated runs with host PulseAudio/Pipewire audio pass-through.
- **Multimodal Agent-Ready**: Integrates XDG-compliant, structured agent skills (`music-discovery`, `audio-analysis`, `listening-companion`) enabling multimodal LLMs to "listen" and curate your audio.

---

## Installation & Setup

Build the project from source (requires Rust toolchain pre-installed):

```bash
# Clone the repository
git clone https://github.com/Xuepoo/agent-lx-music.git
cd agent-lx-music

# Build release target
cargo build --release

# Run global help
./target/release/alx --help
```

---

## Quick Start Reference

```bash
# 1. Register a music source script
alx source add ./my-sixyin-source.js

# 2. Search across platforms (returns dynamic short CLI IDs)
alx search "周杰伦 晴天"

# 3. Play the resolved song via detached mpv daemon
alx play <cli_id>

# 4. Control playback asynchronously
alx now                    # Show real-time progress card
alx volume +10 / alx volume -10
alx seek +30 / alx seek 2:30
alx pause / alx resume / alx stop
alx quit                   # Terminate the mpv daemon cleanly

# 5. Retrieve lyrics & cover art
alx lyric <cli_id>         # Print synchronized LRC lyrics
alx lyric <cli_id> --save  # Export to .lrc file in download folder
alx pic <cli_id> --save    # Download album cover with magic bytes validation
```

---

## Documentation

All technical details, architectural blueprints, and contracts are located in the `docs` directory:
- [Requirements Spec](docs/REQUIREMENTS.md) — Comprehensive feature breakdown
- [Technical Architecture](docs/ARCHITECTURE.md) — Module decoupling and mpv IPC design
- [CLI Reference Manual](docs/CLI.md) — Detailed subcommand and flag options
- [Source Bridge API](docs/SOURCE-API.md) — JS engine execution contract
- [XDG Path Configuration](docs/CONFIG.md) — Environment variables and path resolution
- [SQLite Schema Model](docs/DATA-MODEL.md) — DB schema layout

---

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

### Disclaimer & Supplementary Agreement
Please read our [Project Agreement & Disclaimer](DISCLAIMER.md) for terms of use, third-party source guidelines, copyright compliance, and non-commercial exploration rules.
