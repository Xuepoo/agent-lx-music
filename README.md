# agent-lx-music

[English](README.en.md) | [简体中文](README.md)

基于 Unix 哲学设计的高性能命令行音乐播放器，由 Rust 强力驱动，完美兼容 lx-music 音源脚本。项目彻底抛弃了臃肿的 Electron 框架，改用高度优化的 QuickJS 沙箱环境运行 JS 解析脚本，并通过脱钩的 POSIX 守护进程组（`setsid`）将音频高保真解码与播放工作完全委托给 headless `mpv` 实例处理。

---

## 核心特性

- **QuickJS 隔离沙箱**：基于 [rquickjs](https://github.com/DelSkayn/rquickjs)，在安全隔离的沙箱环境内运行传统的 `lx-music` 音源解析脚本。
- **脱钩式守护进程设计**：利用 POSIX `setsid` 机制在独立的后台进程组中拉起 `mpv`，实现非阻塞 of 音频控制流，命令行退出后后台音乐依然能稳定播放。
- **SQLite 透明数据库缓存**：本地保存歌单、支持年龄限制自动清理的播放历史、收藏夹，并透明地对已解析歌词进行本地缓存，实现零延迟、零网络请求的二次秒开。
- **静态歌词与封面图处理**：支持主歌词、翻译歌词、罗马音轨道的格式化 LRC 快速输出与文件导出；基于魔法字节（Magic Bytes）检测图像文件头签名，规避不稳定 MIME 报头并精确自动补全后缀名。
- **音频直通式容器部署**：深度兼容无根（rootless）Podman / Docker 容器化部署，可通过卷映射直通宿主机 PulseAudio/Pipewire 音频通道。
- **大模型 Agent 智能驱动**：预置了符合 XDG 规范的智能技能文件（`music-discovery`、`audio-analysis`、`listening-companion`），完美适配多模态大语言模型（如 Gemini 1.5 Pro）直接对歌曲进行分析、检索与音乐伴侣闲聊。

---

## 快速安装与配置

从源码进行本地编译（需要预先安装 Rust 工具链）：

```bash
# 克隆代码仓库
git clone https://github.com/Xuepoo/agent-lx-music.git
cd agent-lx-music

# 编译 release 生产包
cargo build --release

# 查看全局帮助文档
./target/release/alx --help
```

---

## 基础命令速查

```bash
# 1. 注册音乐解析源脚本
alx source add ./my-sixyin-source.js

# 2. 全网多平台歌曲搜索 (返回动态生成的短 CLI ID)
alx search "周杰伦 晴天"

# 3. 通过后台守护进程启动歌曲播放
alx play <cli_id>

# 4. 异步多媒体播放状态控制
alx now                    # 展示实时的播放进度卡片
alx volume +10 / alx volume -10
alx seek +30 / alx seek 2:30
alx pause / alx resume / alx stop
alx quit                   # 彻底优雅关闭 mpv 后台守护进程

# 5. LRC 歌词与封面图获取
alx lyric <cli_id>         # 打印同步的 LRC 歌词
alx lyric <cli_id> --save  # 自动导出为 .lrc 文件到下载文件夹
alx pic <cli_id> --save    # 下载专辑封面图并自动修正文件后缀
```

---

## 技术文档导览

所有底层的设计规格、接口协议与数据模型均存放于 `docs` 目录（位于代码仓库的父目录中）：
- [功能规格要求](docs/REQUIREMENTS.md) — 详尽的功能细分与里程碑划分
- [技术架构蓝图](docs/ARCHITECTURE.md) — 模块解耦与 mpv IPC 通信设计
- [CLI 使用手册](docs/CLI.md) — 每一个子命令与选项的配置说明
- [音源桥接 API 规范](docs/SOURCE-API.md) — 沙箱环境内音源事件的回调契约
- [XDG 路径配置指南](docs/CONFIG.md) — 环境变量优先级与路径解析规则
- [SQLite 数据模型](docs/DATA-MODEL.md) — 完整的数据表关系与视图拓扑

---

## 许可证与免责声明

本项目基于 MIT 许可证开源。

### 免责声明与补充协议
请务必阅读并签署 [项目协议与免责声明](docs/DISCLAIMER.zh-CN.md) 了解关于第三方音乐源使用限制、版权数据处理（24小时内清空）、非商业探索性质及技术学习交流的具体协议条款。
