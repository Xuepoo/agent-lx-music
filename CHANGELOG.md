# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.2.5]: https://github.com/Xuepoo/agent-lx-music/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/Xuepoo/agent-lx-music/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/Xuepoo/agent-lx-music/compare/v0.2.1...v0.2.3
[0.2.1]: https://github.com/Xuepoo/agent-lx-music/compare/v0.1.1...v0.2.1
[0.1.1]: https://github.com/Xuepoo/agent-lx-music/releases/tag/v0.1.1
