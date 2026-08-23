# CLI 命令行工具使用手册

`alx` 提供了一套遵循 Unix 哲学的紧凑、直观且强大的命令行指令体系。所有命令均支持使用 `--json` 参数输出机器可读的 JSON 格式数据，并支持 `--quiet` 参数抑制非必要提示信息。

## 全局选项

```bash
alx [OPTIONS] <COMMAND>

选项:
  -c, --config <FILE>    指定配置文件路径 [默认: ~/.config/agent-lx-music/config.toml]
  -q, --quiet            静默模式，抑制所有非核心数据的交互式提示
      --json             强制将输出格式化为 JSON 字符串
      --no-color         禁用终端 ANSI 染色输出
  -v, --verbose          启用冗长日志模式 (用于调试调试)
  -h, --help             打印全局帮助菜单
  -V, --version          打印程序当前版本
```

---

## 1. 音源管理 (Source Management)

### `alx source add <PATH_OR_URL>`

添加本地或远程 JavaScript 音频解析源脚本。

```bash
alx source add ./my-source.js
alx source add https://raw.githubusercontent.com/.../latest.js
alx source add ./my-source.js --name "自定义六音音源"
```

选项：

* `--name <NAME>` — 覆盖音源在系统内的默认显示名称
* `--no-validate` — 强制跳过添加时的脚本运行沙箱合规性校验

### `alx source list`

列出本地已注册安装的所有音源脚本。

```bash
alx source list
alx source list --json
```

### `alx source remove <ID>`

注销并删除已安装的指定音源。

```bash
alx source remove custom_source_v1.0.0
alx source remove custom_source_v1.0.0 --yes
```

### `alx source update [ID]`

检查并更新已配置 URL 的音源脚本。

```bash
alx source update custom_source_v1.0.0     # 更新特定音源
alx source update --all                    # 检查并更新所有有远程 URL 的音源
```

### `alx source info <ID>`

展示指定音源的完整沙箱元数据及支持的平台信息。

```bash
alx source info custom_source_v1.0.0
alx source info custom_source_v1.0.0 --json
```

---

## 2. 全网检索 (Search)

### `alx search <KEYWORD>`

跨已启用音源平台检索歌曲，自动为搜索结果映射 4 位的临时 CLI ID。

```bash
alx search "晴天 周杰伦"
alx search "晴天" --source platformA
alx search "晴天" --page 2
alx search "晴天" --limit 50
alx search "晴天" --json
alx search "晴天" --id-only
```

选项：

* `-s, --source <SOURCE>` — 过滤检索的音乐平台标识符：`all`（并行检索所有平台）、或者自定义 JS 音源脚本注册的具体平台标识符（如 `platformA`、`platformB` 等） [默认: all]
* `-p, --page <N>` — 搜索结果翻页码 [默认: 1]
* `-l, --limit <N>` — 单页返回条目数量 (最大 100) [默认: 30]
* `--id-only` — 仅输出 4 位 CLI ID，每行一个，便于 Shell 管道脚本组合

---

## 3. 音频播放与后台控制 (Playback)

### `alx play <ID_OR_URL_OR_PATH>`

播放目标音频。可以是检索出的 4 位 CLI ID、本地音乐文件路径，或是直链音频网络 URL。

```bash
alx play abc123                        # 播放搜索出来的临时 CLI ID
alx play abc123 --quality flac        # 请求特定的无损音质
alx play https://example.com/song.mp3  # 直接播放网络直链
alx play ~/Music/song.mp3             # 播放本地音频文件
alx play abc123 def456 ghi789          # 传入多首歌曲，直接加载入临时队列顺序播放
alx play --from-playlist "我的收藏"    # 直接载入指定歌单并播放
```

选项：

* `-q, --quality <QUALITY>` — 指定解析音质：`128k`, `320k`, `flac`, `flac24bit`
* `--from-playlist <NAME>` — 载入指定本地歌单并载入播放队列
* `--shuffle` — 载入多首或歌单时，开启随机打乱机制

### `alx now`

输出当前后台守护进程播放的实时多媒体进度状态卡片。

```bash
alx now
alx now --json
```

输出卡片效果示例：

```text
♫ 晴天 — 周杰伦
  Album: 叶惠美 | Source: kw | Quality: 320k
  [████████████░░░░░░░░] 02:15 / 04:29  Vol: 80%
```

### `alx pause`

暂停播放工作（非阻塞后台 mpv 挂起）。

### `alx resume`

恢复后台挂起的播放进度。

### `alx stop`

停止当前音频并清空播放器播放位置。

### `alx next`

切歌，播放下一首。

### `alx prev`

切歌，播放上一首。

### `alx volume <VALUE>`

查询或调整 mpv 播放器音量百分比 (0-100)。

```bash
alx volume 80          # 直接设置为 80%
alx volume +10         # 音量增加 10%
alx volume -10         # 音量减少 10%
alx volume             # 仅查询当前实时音量
```

### `alx seek <VALUE>`

控制当前歌曲进度的快速跳转定位。

```bash
alx seek +30           # 快进 30 秒
alx seek -10           # 快退 10 秒
alx seek 2:30          # 跳转到 2 分 30 秒位置
alx seek 50%           # 精确跳转到歌曲 50% 进度处
```

### `alx repeat <MODE>`

配置循环播放模式。

```bash
alx repeat off         # 顺序播放，队尾停止
alx repeat one         # 单曲循环
alx repeat all         # 整个播放队列列表循环
alx repeat             # 仅查询当前的循环配置
```

### `alx shuffle`

开启、关闭或翻转随机播放选项。

```bash
alx shuffle on
alx shuffle off
alx shuffle            # 翻转随机状态 (开 <-> 关)
```

### `alx quit`

彻底干净地通知 mpv 后台守护进程断开连接，关闭套接字并完美退出。

---

## 4. 队列管理 (Queue)

### `alx queue` / `alx q`

展示当前内存中正在轮转的实时播放队列。

```bash
alx queue
alx queue --json
```

### `alx queue add <ID>`

将检索出来的歌曲追加至当前播放队列尾部。

```bash
alx queue add abc123
alx queue add abc123 def456
```

### `alx queue insert <ID>`

插队播放，将指定歌曲插入至当前正在播放的歌曲之后（即下一首播放）。

### `alx queue remove <POSITION>`

移出队列，指定队列索引值移出歌曲。

```bash
alx queue remove 3     # 移出当前队列中第 3 个位置的歌曲
```

### `alx queue clear`

清空当前的播放队列。

### `alx queue move <FROM> <TO>`

重新调整队列内部的排序位置。

```bash
alx queue move 5 2     # 将第 5 首拖动到第 2 首播放
```

---

## 5. 歌单业务 (Playlists)

### `alx playlist` / `alx pl`

列出本地 SQLite 中保存的所有用户歌单信息。

### `alx playlist create <NAME>`

创建一个全新的本地歌单。

```bash
alx playlist create "跑步电台"
```

### `alx playlist show <NAME>`

展示指定歌单中所存储的全部歌曲明细列表。

```bash
alx playlist show "跑步电台"
alx playlist show "跑步电台" --json
```

### `alx playlist delete <NAME>`

删除一个本地歌单。

```bash
alx playlist delete "跑步电台" --yes
```

### `alx playlist rename <OLD> <NEW>`

重命名指定歌单名字。

### `alx playlist add <NAME> <ID>`

将指定检索歌曲 CLI ID 加密存入指定歌单中。

```bash
alx playlist add "跑步电台" abc123 def456
```

### `alx playlist remove <NAME> <ID>`

从指定歌单中移除一首歌曲。

### `alx playlist play <NAME>`

一键把歌单整体灌入后台播放队列并直接开始启动播放。

```bash
alx playlist play "跑步电台"
alx playlist play "跑步电台" --shuffle
```

### `alx playlist export <NAME>`

将歌单导出至通用的标准多媒体播放列表文件。

```bash
alx playlist export "跑步电台" --format m3u
alx playlist export "跑步电台" --format json --output ~/my-playlists/
```

### `alx playlist import <FILE>`

从外部标准多媒体歌单文件解析并导入到本地 SQLite 数据库中。

```bash
alx playlist import ~/my-playlists/workout.m3u --name "跑步电台"
alx playlist import playlist.json
```

---

## 6. 个人收藏与历史 (Favorites & History)

### `alx fav` / `alx favorites`

查看本地 “我的收藏” 默认预置歌单中的所有歌曲。

### `alx fav add [ID]`

将指定歌曲添加至 “我的收藏”。如果缺省 `ID`，则自动获取并收藏当前后台正在播放的歌曲。

### `alx fav remove <ID>`

取消对指定歌曲的收藏。

### `alx fav play`

直接快速顺序或随机播放所有“我的收藏”中的歌曲。

### `alx history` / `alx hist`

查询用户的本地播放历史记录（带时长和相对时间标记）。

```bash
alx history
alx history --limit 20
alx history --json
```

---

## 7. 离线下载与媒体元数据注入 (Download)

### `alx download <ID>`

极速并行下载指定歌曲并自动注入 ID3v2 封面及多语言同步 LRC 歌词轨道。

```bash
alx download abc123
alx download abc123 --quality flac
alx download abc123 --output ~/Music/
alx download abc123 def456 ghi789
```

选项：

* `-q, --quality <QUALITY>` — 指定下载音质 [默认: 优先读取 config 配置文件]
* `-o, --output <DIR>` — 指定本地落地保存文件夹 [默认: 优先读取 config 配置文件]
* `--no-lyrics` — 禁止在音频内注入或生成外挂歌词文件
* `--no-cover` — 禁止拉取封面并嵌入音频容器内
* `--filename <TEMPLATE>` — 自定义本地落地文件名模板 [默认: `{singer} - {title}.{ext}`]

*文件名模板可用插值变量：`{title}` (歌名), `{singer}` (歌手), `{album}` (专辑), `{id}` (平台歌ID), `{source}` (来源平台), `{ext}` (音频后缀)*

---

## 8. 歌词与封面图直取 (Lyrics & Cover)

### `alx lyric <ID>`

拉取指定音频的歌词内容，智能执行本地 SQLite 缓存查询或远程 JS 解析。

```bash
alx lyric abc123                   # 输出排版好的 LRC 歌词
alx lyric abc123 --translated      # 优先输出译文轨道歌词
alx lyric abc123 --save            # 将歌词以同名独立 .lrc 格式文件导出到下载目录
alx lyric abc123 --json            # JSON 格式输出
```

### `alx pic <ID>`

提取当前音频关联的专辑封面艺术图。

```bash
alx pic abc123                     # 打印专辑封面图片直链 URL
alx pic abc123 --save              # 自动下载图片并使用 Magic Bytes 检测类型写入后缀名
alx pic abc123 --save --output ~/my-covers/
```

---

## 9. 探索与排行榜 (Leaderboards & Explore)

### `alx board` / `alx hot`

拉取并浏览各官方音乐平台的原生排行榜与实时热榜。

```bash
alx board                          # 打印所有支持的音乐榜单
alx board --source platformA              # 单独拉取指定音乐平台当前的所有榜单
alx board --source platformA --id <bid>   # 精确检索列出该平台指定榜单内的歌曲
alx board --source platformA --id <bid> --play  # 一键播放该排行榜所有曲目
```

### `alx discover` / `alx explore`

发现推荐，浏览各平台上由官方或听众精选推荐的歌单。

```bash
alx discover                       # 浏览当前各大平台主页首发的推荐歌单
alx discover --source platformA           # 指定平台的歌单推荐
alx discover --source platformA --tag 华语  # 按照分类标签过滤歌单
alx discover show <playlist-id>    # 列出该推荐歌单内包含的明细歌曲
alx discover play <playlist-id>    # 一键顺序播放该歌单
```

---

## 10. 配置管理 (Config)

### `alx config`

打印或修改当前的系统核心全局参数配置。

```bash
alx config                         # 查看全局 TOML 解析后参数
alx config --json                  # 转换为 JSON 打印

alx config get default_quality     # 快速检索具体 Key 的参数
alx config set default_quality flac # 写入修改 Key 参数值

alx config path                    # 打印 config.toml 配置文件物理路径
alx config edit                    # 直接使用终端的 $EDITOR 打开配置文件进行编辑
```
