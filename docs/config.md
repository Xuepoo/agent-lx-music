# XDG 路径与核心参数配置指南

`alx` 在设计上严格遵循了 XDG 基本目录规范 (XDG Base Directory Specification)，将配置文件、本地数据库以及缓存等资产干净、安全地在宿主机中进行隔离管理。

## 1. 默认物理路径规划 (XDG Compliant)

对于标准的 Linux/Unix 环境，默认的物理存储路径结构如下：

```text
配置文件根目录: ~/.config/agent-lx-music/
  └── config.toml                         # 核心配置文件

数据存储根目录: ~/.local/share/agent-lx-music/
  ├── agent-lx-music.db                  # SQLite 数据库文件 (包含播放列表、历史等)
  └── sources/                            # 自定义 JS 音源脚本文件夹
      ├── custom_source_v1.0.0.js
      └── custom_plugin.js

临时缓存根目录: ~/.cache/agent-lx-music/
  └── (临时 mpv socket 或下载缓存分片文件)
```

### 全局重定向环境变量：`ALX_HOME`

如果您希望将所有配置文件及数据库落地在一个完全隔离或便携式的物理 U 盘路径中，您可以通过设置 `ALX_HOME` 环境变量来实现完全重定向。
例如，执行 `export ALX_HOME=/tmp/alx-isolated` 后，上述所有 XDG 子路径都将自动重映射至 `/tmp/alx-isolated/.config` 等子文件夹中。

---

## 2. 全量配置参数参考 (`config.toml`)

当项目首次启动时，它会在上述位置为您自动生成一份全中文注释、带有默认推荐配置的 `config.toml` 文件：

```toml
# ─── 1. 音频源与解析设置 (Source Settings) ─────────────────────────

[source]
# 默认搜索的平台: "all" (并行多平台检索并合并)，或指定具体自定义音源平台标识符
default_source = "all"

# 默认请求的音频解析音质
# 允许的值: "128k", "192k", "320k", "flac", "flac24bit", "ape", "wav"
default_quality = "320k"

# 降级备用音质链 —— 当默认音质因版权等原因缺失时，会按照数组定义的顺序尝试降级请求其他音质
quality_fallback = ["320k", "128k", "flac"]

# 优先执行自定义 JS 脚本音源：当本地已注册的 JS 脚本与内置 Rust 解密器拥有重叠平台时，是否优先调用 JS 脚本进行解析
js_priority = true

# 自定义音源的解析调用优先级顺序，未列出的脚本音源将默认在尾部以字母表顺序载入
priority = ["custom_source_v1.0.0", "custom_plugin"]


# ─── 2. 播放器底层配置 (Player Settings) ─────────────────────────

[player]
# 后台 mpv 启动时的初始音量百分比 (0-100)
default_volume = 80

# 后台 mpv IPC 通信控制的 Local Socket 套接字物理路径 (留空则由系统在临时文件夹中自动分配与回收)
# mpv_socket = "/tmp/agent-lx-music-mpv.sock"

# 额外追加给 mpv 后台实例启动的自定义参数 (用于发烧友高级音频硬件输出映射微调)
mpv_args = ["--audio-device=alsa/default"]

# 默认循环状态: "off" (队尾停止), "one" (单曲循环), "all" (队列循环)
repeat = "off"

# 默认是否打乱顺序随机播放
shuffle = false


# ─── 3. 音频离线下载设置 (Download Settings) ───────────────────────

[download]
# 下载音频保存的物理物理路径
output_dir = "~/Music/agent-lx-music"

# 本地落地音频文件名模板 (无需加上音频扩展名，系统会自动智能修正写入)
# 可用变量插值: {title} (歌名), {singer} (歌手), {album} (专辑), {id} (歌曲ID), {source} (平台)
filename_template = "{singer} - {title}"

# 下载完音频后的元数据标签智能注入总开关
embed_metadata = true

# LRC 歌词与专辑封面注入设置
embed_lyrics = true               # 在音频容器的 ID3v2/Vorbis 歌词轨道中自动写入 LRC 歌词
embed_lyrics_lx = true            # 在音频中注入更高级的逐字 (Word-by-Word) 动感歌词数据
embed_lyrics_translated = false   # 强制将译文歌词一同合并写入音频
embed_lyrics_romanized = false    # 强制将罗马音歌词一同合并写入音频

# 独立外挂歌词文件导出
save_lyrics_file = false          # 下载音频的同时，在同目录下生成一份独立的同名 .lrc 文件
lrc_encoding = "utf8"             # 独立歌词文件的字符集编码: "utf8" 或 "gbk"

# 专辑封面处理
embed_cover = true                # 将专辑封面以 Cover Art ID3 形式写入音频头部
save_cover_file = false           # 下载音频的同时，在同目录下保存一份同名 cover.jpg 大图

# 下载性能行为控制
max_concurrent = 3                # 允许最大并行下载任务数
skip_existing = true              # 如果本地对应目标路径下文件已存在，则静默跳过
use_other_source = true           # 当选定的当前解析音源下载失效时，自动跨平台查找同名同歌手的可用音频降级补充
group_by_source = false           # 是否在下载目录下为每个自定义音源平台建立独立的子文件夹 (如 platformA/, platformB/)
timeout = 60                      # 下载单个分片的网络超时时间 (秒)


# ─── 4. 播放历史控制 (History Settings) ─────────────────────────

[history]
# 播放历史记录的自动清除年龄 (天数)，配置为 0 则为永久保留历史记录不清除
max_age_days = 90


# ─── 5. UI 显示与终端调色 (Display Settings) ─────────────────────

[display]
# 是否开启终端 ANSI 染色: "auto" (在管道输出中自动关闭染色), "always", "never"
color = "auto"

# 终端输出列表表格排版样式: "rounded" (圆角框), "ascii" (普通加号减号线框), "compact" (无框紧凑)
table_style = "rounded"

# 是否在终端前台显示详细的字符下载进度条
show_progress = true


# ─── 6. 网络代理与性能调节 (Network Settings) ─────────────────────

[network]
# 全局网络 HTTP/HTTPS 代理配置 (支持 socks5 协议)
# proxy = "socks5://127.0.0.1:1080"

# 全网 API 请求的超时配置 (秒)
timeout = 15

# 当碰到瞬态网络抖动失败时，允许 API 自动重试最大次数
max_retries = 2
```

---

## 3. 配置参数解析优先级

当宿主执行一个命令时，配置参数的加载遵循以下漏斗层级关系进行覆盖（底部优先级最高）：

1. **代码硬编码缺省安全值 (Hardcoded Defaults)** — 最底层的安全线。
2. **`config.toml` 配置文件中的配置项** — 全局用户设定。
3. **系统环境变量** — 宿主机环境注入。
4. **CLI 命令行选项与 Flags** — 最高优先级覆盖 (例如命令行显式指定 `-c /tmp/config.toml` 或 `--json`)。

---

## 4. 核心系统环境变量汇总

| 环境变量名 | 说明与生效规则 |
| ---------- | ------------- |
| `ALX_HOME` | 覆写所有默认 XDG 根目录路径的基准路径 |
| `ALX_CONFIG` | 精确指定全局配置文件的物理绝对路径 |
| `ALX_DATA` | 覆写 SQLite 数据库与音源脚本的保存根目录 |
| `ALX_CACHE` | 覆写临时套接字和运行缓存目录 |
| `ALX_MPV_SOCKET` | 手动覆写 mpv IPC Local Socket 的物理路径 |
| `ALX_MPV_BIN` | 覆写要拉起的 mpv 执行文件名 (默认从系统 PATH 寻找 `mpv`) |
| `HTTP_PROXY` / `HTTPS_PROXY` | 传统的 Linux 终端代理环境变量，在 `config.toml` 缺省时自动生效 |
| `NO_COLOR` | 遵循现代 CLI 开源规范，设置此变量后，全程序自动进入无染色纯文本输出模式 |

---

## 5. 项目首次运行的自引导引导行为 (First Run Bootstrap)

当用户在编译或下载后首次在终端里敲击 `alx` 启动时，程序会自动检测并执行以下自引来自引导设置工作：

1. 在用户主目录下自动构建 `~/.config/agent-lx-music/` 配置文件目录。
2. 在上述目录下写入一份全中文注释的示范 `config.toml` 文件。
3. 在用户数据目录下自动构建 `~/.local/share/agent-lx-music/sources/` 目录，供用户存放 JS 脚本。
4. 自动在数据目录下连接并初始化 SQLite 数据库实体，构建播放列表、历史、收藏等表结构。
5. 在终端中输出代表项目成功引导完成的精美控制台彩蛋：

    ```text
    ✓ agent-lx-music 初始化配置成功完成！
      配置文件路径: ~/.config/agent-lx-music/config.toml
      本地数据根目录: ~/.local/share/agent-lx-music/
 
      如何开始您的听歌之旅：
        alx source add <url>    添加您的自定义音源
        alx search <keyword>    全网多平台检索歌曲
        alx play <cli_id>       通过后台守护进程播放音频
    ```
