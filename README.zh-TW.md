# agent-lx-music

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [日本語](README.ja.md) | [Français](README.fr.md) | [Español](README.es.md)

基於 Unix 哲學設計的高性能命令列音樂播放器，由 Rust 強力驅動，完美相容 lx-music 音源腳本。項目徹底拋棄了臃腫的 Electron 框架，改用高度優化的 QuickJS 沙箱環境運行 JS 解析腳本，並通過脫鉤的 POSIX 守護進程組（`setsid`）將音訊高保真解碼與播放工作完全委託給 headless `mpv` 實例處理。

---

## 核心特性

- **QuickJS 隔離沙箱**：基於 [rquickjs](https://github.com/DelSkayn/rquickjs)，在安全隔離的沙箱環境內運行傳統的 `lx-music` 音源解析腳本。
- **脫鉤式守護進程設計**：利用 POSIX `setsid` 機制在獨立的後台進程組中拉起 `mpv`，實現非阻塞的音訊控制流，命令列退出後後台音樂依然能穩定播放。
- **SQLite 透明資料庫快取**：本地保存歌單、支援年齡限制自動清理的播放歷史、收藏夾，並透明地對已解析歌詞進行本地快取，實現零延遲、零網路請求的二次秒開。
- **靜態歌詞與封面圖處理**：支援主歌詞、翻譯歌詞、羅馬音軌道的格式化 LRC 快速輸出與檔案匯出；基於魔法位元組（Magic Bytes）檢測圖像檔案頭簽名，規避不穩定 MIME 報頭並精確自動補全副檔名。
- **音訊直通式容器部署**：深度相容無根（rootless）Podman / Docker 容器化部署，可通過磁碟區對照直通宿主機 PulseAudio/Pipewire 音訊通道。
- **大模型 Agent 智能驅動**：預置了符合 XDG 規範的智能技能檔案（`music-discovery`、`audio-analysis`、`listening-companion`），完美適配多模態大語言模型（如 Gemini 1.5 Pro）直接對歌曲進行分析、檢索與音樂伴侶閒聊。

---

## 快速安裝與配置

從源碼進行本地編譯（需要預先安裝 Rust 工具鏈）：

```bash
# 克隆代碼倉庫
git clone https://github.com/Xuepoo/agent-lx-music.git
cd agent-lx-music

# 編譯 release 生產包
cargo build --release

# 查看全局幫助文檔
./target/release/alx --help
```

---

## 基礎命令速查

```bash
# 1. 註冊音樂解析源腳本
alx source add ./my-sixyin-source.js

# 2. 全網多平台歌曲搜尋 (返回動態生成的短 CLI ID)
alx search "周杰倫 晴天"

# 3. 通過後台守護進程啟動歌曲播放
alx play <cli_id>

# 4. 異步多媒體播放狀態控制
alx now                    # 展示實時的播放進度卡片
alx volume +10 / alx volume -10
alx seek +30 / alx seek 2:30
alx pause / alx resume / alx stop
alx quit                   # 徹底優雅關閉 mpv 後台守護進程

# 5. LRC 歌詞與封面圖獲取
alx lyric <cli_id>         # 打印同步的 LRC 歌詞
alx lyric <cli_id> --save  # 自動匯出為 .lrc 檔案到下載文件夾
alx pic <cli_id> --save    # 下載專輯封面圖並自動修正檔案副檔名
```

---

## 技術文檔導覽

所有底層的设计规格、接口协议與資料模型均存放於 `docs` 目錄（位於代碼倉庫的父目錄中）：
- [功能規格要求](docs/REQUIREMENTS.md) — 詳盡的功能細分與里程碑劃分
- [技術架構藍圖](docs/ARCHITECTURE.md) — 模組解耦與 mpv IPC 通信設計
- [CLI 使用手冊](docs/CLI.md) — 每一個子命令與選項的配置說明
- [音源橋接 API 規範](docs/SOURCE-API.md) — 沙箱環境內音源事件的回調契約
- [XDG 路徑配置指南](docs/CONFIG.md) — 環境變數優先級與路徑解析規則
- [SQLite 資料模型](docs/DATA-MODEL.md) — 完整的資料表關係與視圖拓撲

---

## 許可證

本項目基於 MIT 許可證開源。
