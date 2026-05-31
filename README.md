# agent-lx-music

[English](README.en.md) | [简体中文](README.md)

基于 Unix 哲学设计的高性能命令行音乐播放器，由 Rust 强力驱动，完美兼容 lx-music 音源脚本。项目彻底抛弃了臃肿的 Electron 框架，改用高度优化的 QuickJS 沙箱环境运行 JS 解析脚本，并通过脱钩的 POSIX 守护进程组（`setsid`）将音频高保真解码与播放工作完全委托给 headless `mpv` 实例处理。

---

## 核心特性

- **QuickJS 隔离沙箱**：基于 [rquickjs](https://github.com/DelSkayn/rquickjs)，在安全隔离的沙箱环境内运行传统的 `lx-music` 音源解析脚本。
- **脱钩式守护进程设计**：利用 POSIX `setsid` 机制在独立的后台进程组中拉起 `mpv`，实现非阻塞的音频控制流，命令行退出后后台音乐依然能稳定播放。
- **SQLite 透明数据库缓存**：本地保存歌单、支持年龄限制自动清理的播放历史、收藏夹，并透明地对已解析歌词进行本地缓存，实现零延迟、零网络请求的二次秒开。
- **静态歌词与封面图处理**：支持主歌词、翻译歌词、罗马音轨道的格式化 LRC 快速输出与文件导出；基于魔法字节（Magic Bytes）检测图像文件头签名，规避不稳定 MIME 报头并精确自动补全后缀名。
- **音频直通式容器部署**：深度兼容无根（rootless）Podman / Docker 容器化部署，可通过卷映射直通宿主机 PulseAudio/Pipewire 音频通道。
- **大模型 Agent 智能驱动**：预置了符合 XDG 规范的智能技能文件（`music-discovery`、`audio-analysis`、`listening-companion`），完美适配多模态大语言模型（如 Gemini 1.5 Pro）直接对歌曲进行分析、检索与音乐伴侣闲聊。

---

## 快速上手 (Quick Start)

### 1. 安装外部系统依赖 (External Dependencies)

`alx` 依赖底层的系统音频输出驱动与高保真解码后端。在编译和运行本项目之前，您需要安装以下外部依赖项：

*   **`mpv`** *(核心必须)*：充当 headless 播放解调后端。
*   **`libmpv-dev` (或 `mpv-devel`)** *(编译必须)*：供 Rust 核心原生链接和调用 mpv 进程间套接字。
*   **`alsa-lib` (或 `libasound2-dev`)** *(Linux 必须)*：用于对接底层 ALSA 发声通道。
*   **`beets`** *(可选依赖)*：当您需要将本播放器与本地个人音乐库进行同步管理时使用。

#### 主流操作系统/发行版安装命令一览：

*   **Debian / Ubuntu / Deepin / Mint**:
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

### 2. 获取并本地编译 (Build from Source)

环境依赖安装就绪后，直接克隆并编译本项目：

```bash
# 1. 克隆代码仓库
git clone https://github.com/Xuepoo/agent-lx-music.git
cd agent-lx-music

# 2. 编译 Release 生产二进制包 (默认已开启 native-all 特性以包含全平台原生搜索引擎)
cargo build --release

# 3. 查看全局使用菜单
./target/release/alx --help
```
*(注意：您也可以直接前往 GitHub 的 [Releases](https://github.com/Xuepoo/agent-lx-music/releases) 页面，下载开箱即用的预编译二进制或 `.deb` / `.rpm` / `.apk` 安装包。)*

---

### 3. 三步极速配置开歌 (Bootstrap)

```bash
# 第一步：获取并注册一个自定义的外部解析源脚本
alx source add https://example.com/custom-source.js

# 第二步：全网搜索您想听的音乐
alx search "周杰伦 晴天"
# 检索将返回一个 4 位的临时 CLI ID (如 c12a)

# 第三步：顺序载入并开始顺序后台顺序后台播放
alx play c12a
```

---

## 基础命令速查

```bash
# 1. 异步多媒体播放状态控制
alx now                    # 展示实时的播放进度卡片
alx volume +10 / alx volume -10
alx seek +30 / alx seek 2:30
alx pause / alx resume / alx stop
alx quit                   # 彻底优雅关闭 mpv 后台守护进程

# 2. LRC 歌词与封面图获取
alx lyric <cli_id>         # 打印同步的 LRC 歌词
alx lyric <cli_id> --save  # 自动导出为 .lrc 文件到下载文件夹
alx pic <cli_id> --save    # 下载专辑封面图并自动修正文件后缀
```

---

## 技术文档导览

本项目的全部核心设计细节、参数及数据持久化模型，均存放于 `docs/` 目录中：
- [CLI 使用手册](docs/cli.md) — 每一个子命令与全局选项的配置与用法说明
- [音源桥接 API 规范](docs/source-api.md) — 沙箱隔离环境内音源事件的回调契约
- [XDG 路径配置指南](docs/config.md) — 环境变量优先级与路径解析规则
- [SQLite 数据模型](docs/data-model.md) — 完整的本地关系数据表实体与视图设计

---

## 许可证与免责声明

本项目基于 MIT 许可证开源。

### 免责声明与补充协议
请务必阅读并签署 [项目协议与免责声明](docs/disclaimer.zh-cn.md) 了解关于第三方音乐源使用限制、版权数据处理（24小时内清空）、非商业探索性质及技术学习交流的具体协议条款。
