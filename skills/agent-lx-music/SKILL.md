---
name: agent-lx-music
description: "Control agent-lx-music (alx) CLI to search, play, download music, fetch lyrics/covers, and manage playlists."
version: 1.0.0
author: agent-lx-music project
license: MIT
metadata:
  hermes:
    tags: [music, cli, playback, download, playlist, lyric, cover]
    related_skills: [mpv-media-player]
---

# agent-lx-music (alx) — Music CLI Agent Usage Guide

## Overview

`alx` is the high-performance music CLI engine for `agent-lx-music`. It is completely terminal-native and operates on a decoupled architecture using `mpv` inside a POSIX detached daemon. Agents use `alx` to automate search queries, control media states via IPC sockets, execute high-speed parallel downloads, cache lyrics, and extract album cover art.

All commands support `--json` output, allowing seamless piping and structured automation.

---

## Quick Command Reference

```bash
# 1. Global Custom Configuration (Dynamically Propagated)
alx --config <toml_path> <COMMAND>      # Override config path globally for this run

# 2. Search Music
alx search "周杰伦 晴天"                 # Search across all enabled platforms
alx search "晴天" --source wy            # Search NetEase Music ('wy') only
alx search "晴天" --id-only              # Output only dynamic short hashes (ideal for xargs)
alx search "晴天" --json                 # Structured JSON output

# 3. Media Playback Control
alx play <cli_id_or_url>                 # Play a cached song ID, direct URL, or local file
alx play <id> --quality flac             # Request specific audio resolution quality
alx now                                  # Renders active progress bar, volume, and metadata
alx pause                                # Pause playback
alx resume                               # Resume playback
alx stop                                 # Stop audio stream
alx volume +10 / alx volume -10          # Adjust volume relatively (0-100)
alx seek +30 / alx seek 2:30             # Seek relatively or absolutely
alx repeat off|one|all                   # Cycle through repeat states
alx shuffle on|off                       # Toggle shuffle status
alx quit                                 # Terminate background mpv daemon cleanly

# 4. Playlist & Favorites
alx fav add                              # Register active song to Favorites playlist
alx fav list                             # Output tabular or JSON favorites list
alx fav play --shuffle                   # Play loaded favorites shuffled
alx playlist create "Coding"             # Create custom playlist
alx playlist add "Coding" <cli_id>       # Add track to playlist

# 5. Parallel Downloading
alx download <cli_id>                    # High-speed download with ID3 tags embedding
alx download <cli_id> -o /path/to/dir    # Output to custom directory

# 6. Static LRC Lyrics & Cover Art (Phase 6 Specs)
alx lyric <cli_id>                       # Print main lyrics instantly (SQLite-cached)
alx lyric <cli_id> --translated          # Print translated lyrics
alx lyric <cli_id> --romanized           # Print romanized phonetic lyrics
alx lyric <cli_id> --save                # Auto-save track as .lrc file in download folder
alx pic <cli_id>                         # Output direct URL of album cover art
alx pic <cli_id> --save                  # Download cover using User-Agent with Magic Bytes Detection
alx pic <cli_id> --save -o <path>        # Download cover to custom directory/file path
```

---

## High-Performance Automation Patterns

### Pattern 1: Intelligent Search → Play Match
Query search results, extract the primary item's CLI ID using `jq`, and initialize playback:
```bash
# Search and play the top NetEase match
CLI_ID=$(alx search "周杰伦 晴天" --source wy --json | jq -r '.list[0].id')
alx play "$CLI_ID"
```

### Pattern 2: Bulk Playlist Caching & Downloading
Retrieve all tracks in a user list and download them concurrently in flac resolution:
```bash
alx playlist export "Favorites" --json | jq -r '.list[].id' | xargs -P 4 -I {} alx download {} --quality flac
```

### Pattern 3: Auto-Export Lyrics to LRC files
Grab the currently playing song's metadata, download its main/translated lyrics, and output them to download folders:
```bash
# Save main and translated lyrics for current song
alx lyric --save
alx lyric --translated --save
```

---

## Output Protocols (For JSON Piping)

### JSON search result output (`alx search <query> --json`)
```json
{
  "list": [
    {
      "id": "c1a2",
      "name": "晴天",
      "singer": "周杰伦",
      "source": "kw",
      "interval": "04:29",
      "album_name": "叶惠美",
      "pic_url": "http://img.music.com/cover.png"
    }
  ],
  "total": 42,
  "page": 1
}
```

### JSON lyrics output (`alx lyric <id> --json`)
```json
{
  "song_id": "04164d6d",
  "cli_id": "c1a2",
  "name": "晴天",
  "singer": "周杰伦",
  "track": "main",
  "lyric": "[00:00.00]晴天 - 周杰伦\n[00:29.00]故事的小黄花\n..."
}
```

### JSON cover art output (`alx pic <id> --json`)
```json
{
  "song_id": "04164d6d",
  "cli_id": "c1a2",
  "name": "晴天",
  "singer": "周杰伦",
  "pic_url": "http://img.music.com/cover.png"
}
```

---

## XDG Paths & Environment Variables

To write config, state, or cache scripts under compliance:
- **`ALX_HOME`**: Global override for all configuration/cache directories.
- **`ALX_CONFIG`**: Directly links config file (default: `~/.config/agent-lx-music/config.toml`).
- **`ALX_DATA`**: Data directory (default: `~/.local/share/agent-lx-music/`). Contains SQLite database `agent-lx-music.db`.
- **`ALX_CACHE`**: Cache directory (default: `~/.cache/agent-lx-music/`). Contains daemon IPC socket `mpv.sock` and state `current.json`.
