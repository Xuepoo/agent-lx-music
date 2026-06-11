# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.3.4] - 2026-06-11

### Fixed
- **Playback State Synchronization**: Implemented a real-time state synchronization mechanism using an embedded mpv Lua script. This ensures `current.json` and `queue.json` are immediately updated when songs change, volume is adjusted, or playback is paused/resumed, even when the CLI is not running.
- **Multi-File Playback**: Fixed a bug where `alx play` would only handle the first file or URL provided. Now multiple arguments are correctly queued in mpv and reflected in the local `queue.json`.
- **Unified State Schema**: Unified the data schema for player state tracking, resolving issues where `alx now` and other commands would occasionally fail to display song metadata correctly.
- **Lyric/Cover Extraction**: Updated `alx lyric` and `alx pic` commands to support the new unified state schema when retrieving metadata for the currently playing song.
- **Configuration Consistency**: Removed hardcoded volume defaults; the player now strictly respects the `default_volume` setting in the user's configuration file.

---

## [0.3.3] - 2026-05-31

### Fixed
- **Relative Path Playback**: Resolved relative paths in `alx play` (and downstream `expand_path` pipeline in `config.rs`) by converting any platform relative paths to absolute using `std::env::current_dir()`. This ensures files specified by relative paths (e.g. `alx play file.mp3`) are correctly located by the detached mpv daemon process running from a different working directory.

---

## [0.3.2] - 2026-05-31

### Added
- **Source Health Diagnostics & Circuit Breaker**: Added a new local SQLite metadata table `source_health` to log request successes and failures for custom JavaScript sources. Failing resolver scripts are dynamically circuit-broken after 3 consecutive failures (cooling down for 5 minutes) to protect backend scraper performance.
- **Health Diagnostics CLI Subcommand**: Implemented `alx source health` subcommand to display a beautiful diagnostic dashboard of custom source metrics (Total Requests, Failures, Error Rate, and status tags with vibrant styling). Supports `--json` mapping outputs.
- **Integration Tests Refactoring**: Migrated unit tests from `src/library/playlist_parser.rs` to a dedicated integration tests file `crates/lux-cli/tests/playlist_parser_test.rs`. Cleaned up all Chinese test strings and replaced them with standard English songs ("Sunny Day", "Jay Chou") to ensure internationalized data compliance.

### Fixed
- **SQLite Lock Contention Mitigation**: Refactored the core downloader database progress loop in `download.rs`. Introduced a 250ms & 2.5% progression-based throttle guard to eliminate SQLite `database is locked` error contentions during concurrent search and download daemon operations.
- **Atomic File Placements**: Guaranteed absolute filesystem placement atomicity by storing stream data into temporary `.part` files, flushing them via `sync_all()`, and executing atomic `rename` replacements.
- **Unicode Filename Sanitizer**: Implemented robust path character normalization in `sanitize_filename` (cleaning controls, emojis, and trim formatting while enforcing a strict 180 characters limit) to resolve filesystem writing issues across different host partitions.

---

## [0.3.1] - 2026-05-31

### Added
- **Automatic Quality Fallback**: Implemented custom download fallback algorithm (`get_fallback_qualities`) in `crates/lux-cli/src/cmd/download.rs`. Downloads encountering transient stream/decoding network failures automatically drop down through standard profiles (`flac24bit -> flac -> 320k -> 192k -> 128k`) sequentially.
- **Fallbacked Quality Synchronization**: Integrated database progress syncing. In the event of a successful fallback resolution, the local SQLite database task record is automatically updated to match the final downloaded stream quality.

### Fixed
- **Purge Stale Sources**: Purged expired source IDs (`ikun_v22`, `huibq_lxmusic_v1.2.0`) from local config templates, active XDG configuration scopes, and architectural specification documentation.
- **Crypto Dependencies Adaptations**: Safely migrated `aes` to `0.9.1` and `cbc` to `0.2.1` by refactoring custom Webcrypt bindings in `bridge.rs` to implement `BlockCipherEncrypt` and `BlockModeEncrypt` using modern `encrypt_block_b2b` interfaces.
- **Rand Upgrade Reversal**: Intercepted and blocked breaking `rand v0.9` dependency propagation to avoid structural traits collisions with RSA cryptographic suites.

---

## [0.3.0] - 2026-05-30

### Added
- **Dependency Upgrades**: Upgraded workspace dependencies including `rquickjs` to `0.12.0`, `rusqlite` to `0.40.0`, `md-5` to `0.11.0`, `toml` to `1.1.2`, and `id3` to modern versions.

### Fixed
- **rusqlite 0.40.0 Compilation**: Resolved `usize: ToSql` compile errors in updated `rusqlite` by explicitly casting values to `i64` in `db.rs`.
- **md-5 LowerHex Breakage**: Fixed compile errors across search, source, database, and playlist parsing modules by refactoring `format!("{:x}", ...)` output to use `hex::encode(...)` on `Digest` objects.
- **GitHub Workflows Integration**: Manually upgraded `actions/upload-artifact` to `v4` and `docker/metadata-action` to `v5` inside GitHub Action workflows to bypass OAuth workflow-scope verification failures.

---

## [0.2.5] - 2026-05-30

### Added
- **Default Features Upgrade**: Enabled `native-all` as a default compilation feature in `crates/lux-cli/Cargo.toml`. Standard installations via `cargo install` or `cargo build` now automatically compile all native search engines (Kuwo, Netease, Kugou, Tencent, Migu) out of the box.
- **Search Diagnostic Hint**: Added a conditional compile-time warning in `alx search`. When a search yields zero results and `lux-native` features are not compiled, a user-friendly yellow hint will be printed to guide configuration/re-compilation.
- **GitHub Community Standards**: Project community guidelines initialized including `SECURITY.md`, `CONTRIBUTING.md`, issue templates (`bug_report.yml`, `feature_request.yml`), and a pull request template.

### Fixed
- **JS Source Sandboxing & Obfuscation**: Implemented comprehensive Babel runtime polyfills in `crates/lux-cli/src/source/bridge.rs` (including `regeneratorRuntime`, `_regeneratorRuntime`, `_regeneratorDefine2`, `asyncGeneratorStep`, and `_asyncToGenerator`) to allow highly obfuscated Webpack-transpiled Custom洛雪 Sources to run seamlessly in the QuickJS sandbox.
- **Global Proxy Refactoring**: Refactored the JS bridge to dynamically intercept and proxy direct global invocations (such as calling `request`, `send`, `on`, `utils`, `env`, and `version` directly in the global scope) to `globalThis.lx` with fallback storage. This guarantees backward compatibility with older userApi custom scripts.
- **JS Scraper Priority Sorting**: Resolved a architectural bug in `crates/lux-cli/src/source/mod.rs` where alx ignored the `source.priority` array defined in `config.toml` and queried custom JS sources solely by ID alphabetical order. Scrapers are now dynamically sorted by priority on each resolution pass, avoiding latency from failing high-priority sources (e.g. 500 errors).
- **Cross-Device Link Rename Errors**: Resolved a severe download hanging bug when output directories were configured on a different physical mount or partition (EXDEV, OS Error 18). Implemented a robust fallback routine in `execute_download` that uses copy-and-remove when atomic rename fails.
- **Default Configurations Refreshed**: Updated `Default` implementation and config templates in `config.rs` to deprecate stale references (like `ikun_v22`) and target the modern custom source ecosystem (`_4`, `_9.393DeepSeek`).

---

## [0.2.4] - 2026-05-30

### Fixed
- **Source Testing Skip Logic**: Introduced yellow warnings for unsupported actions in source testing rather than triggering execution errors, improving the stability of custom scraper validation workflows.

---

## [0.2.3] - 2026-05-30

### Added
- **Playback Control Robustness**: Added automatic recovery hooks for mpv connection failures and refined background daemon IPC stability.

---

## [0.2.1] - 2026-05-28

### Added
- **Wasm-based Encryption Bridge**: Introduced local rust-based cryptography support for high-performance hashing and decompression mapping to the QuickJS execution context.

---

## [0.1.1] - 2026-05-25

### Added
- **Initial Release**: Core terminal-native music command-line interface `alx` providing offline local indexing, beets integration, and dynamic script source integration.

[0.3.3]: https://github.com/Xuepoo/agent-lx-music/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/Xuepoo/agent-lx-music/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/Xuepoo/agent-lx-music/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/Xuepoo/agent-lx-music/compare/v0.2.5...v0.3.0
[0.2.5]: https://github.com/Xuepoo/agent-lx-music/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/Xuepoo/agent-lx-music/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/Xuepoo/agent-lx-music/compare/v0.2.1...v0.2.3
[0.2.1]: https://github.com/Xuepoo/agent-lx-music/compare/v0.1.1...v0.2.1
[0.1.1]: https://github.com/Xuepoo/agent-lx-music/releases/tag/v0.1.1
