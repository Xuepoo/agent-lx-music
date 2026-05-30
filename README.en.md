# agent-lx-music

[English](README.en.md) | [简体中文](README.md)

A Unix-philosophy CLI music player powered by custom music sources, written in Rust. It drops Electron entirely, running JS scrapers inside an isolated QuickJS sandbox and delegating high-fidelity audio playback to a headless `mpv` instance over a detached POSIX daemon loop.

---

## Key Features

- **QuickJS Sandbox Integration**: Executes standard custom javascript music sources securely inside a highly optimized [rquickjs](https://github.com/DelSkayn/rquickjs) sandbox environment.
- **Decoupled POSIX Daemonization**: Spawns headless `mpv` playback loops inside detached `setsid` process groups, letting you manage and check playback states asynchronously without locking terminals.
- **SQLite Transparent Caching**: Provisions a local database to store playlists, playback histories (with auto-purges), favorites, and transparently cache LRC lyrics for zero-network lookups.
- **Static LRC Lyrics & Cover Downloads**: High-speed, high-reliability extraction of synchronized lyrics (with translation and romanized fallbacks) and Magic-Bytes based image suffix detection.
- **Audio-Enabled Containerization**: Fully compatible with rootless Podman/Docker, enabling isolated runs with host PulseAudio/Pipewire audio pass-through.
- **Multimodal Agent-Ready**: Integrates XDG-compliant, structured agent skills (`music-discovery`, `audio-analysis`, `listening-companion`) enabling multimodal LLMs to "listen" and curate your audio.

---

## Quick Start

### 1. Install External Dependencies

`alx` requires underlying system audio drivers and a high-fidelity playback engine. Before compiling or running the project, make sure to install the following prerequisites:

*   **`mpv`** *(Core Required)*: Runs in headless daemon mode to stream and decode audio.
*   **`libmpv-dev` (or `mpv-devel`)** *(Compile Required)*: Native C headers for linking Rust's mpv APIs.
*   **`alsa-lib` (or `libasound2-dev`)** *(Linux Required)*: Interface to connect with standard ALSA audio channels.
*   **`beets`** *(Optional)*: Highly recommended if you want to sync, tag, and import metadata from local music libraries.

#### Environment Setup Commands:

*   **Debian / Ubuntu / Mint**:
    ```bash
    sudo apt update
    sudo apt install -y libasound2-dev libmpv-dev mpv beets
    ```
*   **Fedora / RHEL / CentOS**:
    ```bash
    sudo dnf install -y alsa-lib-devel mpv-devel mpv beets
    ```
*   **Arch Linux / Manjaro**:
    ```bash
    sudo pacman -Sy alsa-lib mpv beets
    ```
*   **Alpine Linux**:
    ```bash
    apk add alsa-lib-dev mpv-dev mpv beets
    ```
*   **macOS (via Homebrew)**:
    ```bash
    brew install mpv beets
    ```

---

### 2. Clone & Build from Source

Once the system dependencies are installed, you can clone and build the binary:

```bash
# 1. Clone the repository
git clone https://github.com/Xuepoo/agent-lx-music.git
cd agent-lx-music

# 2. Build optimized release target (native-all feature is enabled by default to compile native scrapers)
cargo build --release

# 3. View global options and command list
./target/release/alx --help
```
*(Note: You can also download pre-built standalone binaries or `.deb` / `.rpm` / `.apk` packages directly from the GitHub [Releases](https://github.com/Xuepoo/agent-lx-music/releases) page.)*

---

### 3. Bootstrap Playback in 3 Steps

```bash
# Step 1: Add a custom JS platform source scraper
alx source add https://example.com/custom-source.js

# Step 2: Search for music across platforms
alx search "周杰伦 晴天"
# The query returns a short 4-character CLI ID (e.g., c12a)

# Step 3: Detach and play the resolved audio in background daemon
alx play c12a
```

---

## Quick Reference Commands

```bash
# 1. Playback & controls
alx now                    # Show real-time playback status progress card
alx volume +10 / alx volume -10
alx seek +30 / alx seek 2:30
alx pause / alx resume / alx stop
alx quit                   # Terminate the mpv backend daemon cleanly

# 2. Retrieve lyrics & cover art
alx lyric <cli_id>         # Fetch and print synchronized LRC lyrics
alx lyric <cli_id> --save  # Export LRC lyric to a local .lrc file in download folder
alx pic <cli_id> --save    # Download album art with magic bytes file suffix validation
```

---

## Documentation

All architectural specs, command options, and data schemas are documented in detail inside the `docs/` directory:
- [CLI Reference Manual](docs/CLI.md) — Detailed subcommand descriptions and global flags
- [Source Bridge API](docs/SOURCE-API.md) — JavaScript bridge execution contracts inside isolated sandboxes
- [XDG Path Configuration](docs/CONFIG.md) — Environment variables, default directories, and resolving rules
- [SQLite Schema Model](docs/DATA-MODEL.md) — SQLite database schema definitions and indexing topological graphs

---

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

### Disclaimer & Supplementary Agreement
Please read our [Project Agreement & Disclaimer](docs/DISCLAIMER.md) for terms of use, third-party source guidelines, copyright compliance, and non-commercial exploration rules.
