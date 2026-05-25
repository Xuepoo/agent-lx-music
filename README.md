# rust-lx

A Unix-philosophy CLI music player powered by lx-music sources, written in Rust.

## Quick Start

```bash
# Install
cargo install rust-lx

# Add a music source
rlx source add https://raw.githubusercontent.com/.../latest.js

# Search and play
rlx search "周杰伦 晴天"
rlx play <song-id>
```

## What is this?

`rust-lx` is a command-line music player that rewrites [lx-music-desktop](https://github.com/lyswhut/lx-music-desktop) as a terminal-native tool. It drops Electron entirely and delegates audio playback to mpv.

**Key design decisions:**
- JS music sources run in a [rquickjs](https://github.com/DelSkayn/rquickjs) sandbox (QuickJS engine)
- Optional native Rust parsers for direct platform API access
- mpv handles all audio playback via JSON IPC
- XDG Base Directory Specification for all paths
- SQLite for local data (playlists, history, favorites)
- Pipe-friendly output (JSON, plain text)

## Project Structure (Cargo Workspace)

```
rust-lx/
├── crates/
│   ├── lux-core/      # Shared types, traits, config
│   ├── lux-native/    # Native Rust platform parsers (kw, kg, wy, tx, mg)
│   └── lux-cli/       # Main binary (rlx), rquickjs sandbox, mpv control
├── docs/              # Specification documents
└── Cargo.toml         # Workspace root
```

## Documentation

- [Requirements](docs/REQUIREMENTS.md) — Full feature specification
- [Architecture](docs/ARCHITECTURE.md) — Technical design and module breakdown
- [CLI Reference](docs/CLI.md) — All commands and usage
- [Source API Contract](docs/SOURCE-API.md) — JS source script interface
- [Configuration](docs/CONFIG.md) — Config file and XDG paths
- [Data Model](docs/DATA-MODEL.md) — SQLite schema and types
- [Native API](docs/NATIVE-API.md) — Rust platform parser design

## License

MIT
