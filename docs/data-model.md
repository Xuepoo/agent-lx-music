# SQLite 数据库与实体数据模型规范

`alx` 在本地使用一个嵌入式的轻量关系型 SQLite 数据库来持久化用户数据。该数据库完美托管了所有的离线歌单、自定义音源注册元数据、播放历史卡片、以及高度优化的本地词图缓存（Lyrics Cache），从物理上规避了任何重复网络请求与 JS 沙箱在开机运行时的初始化开销。

*   **默认数据库落地物理路径**：`~/.local/share/agent-lx-music/agent-lx-music.db`

---

## 1. 实体表结构定义 (Schema Specification)

### 1.1 `sources` —— 自定义 JS 脚本音源元数据表
该表保存已成功注册安装的外部音源脚本的元数据特征，用于在系统启动时在后台沙箱中极速热加载：

```sql
CREATE TABLE IF NOT EXISTS sources (
    id          TEXT PRIMARY KEY,       -- 音源唯一内部 ID (例如: "custom_source_v1.0.0")
    name        TEXT NOT NULL,          -- 脚本头部声明的 @name
    version     TEXT,                   -- 脚本版本号 @version
    author      TEXT,                   -- 作者名 @author
    homepage    TEXT,                   -- 主页链接 @homepage
    repository  TEXT,                   -- 代码托管仓库 URL (用于脚本自动安全升级)
    script_path TEXT NOT NULL,          -- 脚本落地保存的绝对路径 (位于 ~/.local/share/agent-lx-music/sources/ 下)
    source_url  TEXT,                   -- 原始下载获取 URL
    content_hash TEXT NOT NULL,         -- 脚本内容的 MD5 校验码，防止脚本被恶意篡改
    platforms   TEXT NOT NULL,          -- 该音源支持的平台，以 JSON Array 格式存储：["platformA","platformB"]
    qualities   TEXT NOT NULL,          -- 该音源各平台支持的音质，JSON Object 格式存储：{"platformA":["128k","320k"],"platformB":["128k","flac"]}
    enabled     INTEGER NOT NULL DEFAULT 1, -- 该音源是否处于启用状态 (1=启用, 0=禁用)
    created_at  TEXT NOT NULL,          -- 记录建立时间 (标准 ISO 8601 格式)
    updated_at  TEXT NOT NULL           -- 记录最后一次更新时间
);
```

### 1.2 `search_cache` —— 临时搜索词图结果缓存表
为了能够为全网搜索结果映射出极其简短、易于在终端键盘输入的 4 位临时 CLI ID（如 `c12a` 等短码），我们在本地构建了一个带有过期自清理机制的全局搜索词图缓存表：

```sql
CREATE TABLE IF NOT EXISTS search_cache (
    id          INTEGER PRIMARY KEY AUTOINCREMENT, -- 本地自增键，其派生出 4 位 Hex 值的临时播放 ID
    song_id     TEXT NOT NULL,          -- 音乐平台内的原始歌曲 ID
    name        TEXT NOT NULL,          -- 歌名
    singer      TEXT NOT NULL,          -- 歌手/艺术家名字
    source      TEXT NOT NULL,          -- 音频物理来源平台 (如 "platformA", "platformB")
    interval    TEXT,                   -- 歌曲时长排版表示 (如 "03:55")
    album_name  TEXT,                   -- 关联专辑名称
    album_id    TEXT,                   -- 关联专辑 ID
    pic_url     TEXT,                   -- 专辑封面图原始外链
    songmid     TEXT,                   -- 部分平台特定的映射字段
    hash        TEXT,                   -- 酷狗等平台特定的 Hash 校验值
    extra       TEXT,                   -- 平台特定的自定义额外字段插值，以 JSON Blob 文本形式高度弹性保留
    cached_at   TEXT NOT NULL,          -- 记录写入的 ISO 8601 相对时间戳
    UNIQUE(song_id, source)             -- 对平台与歌曲 ID 建立唯一约束，规避冗余行
);

-- 建立检索时效性索引，用于在每次开机时自动对过期缓存行进行极其快速的极速清除
CREATE INDEX IF NOT EXISTS idx_search_cache_cached_at ON search_cache(cached_at);
```

### 1.3 `lyrics_cache` —— 本地歌词秒开缓存表
本项目完美实现了一套透明的本地 SQLite 歌词缓存层。当用户请求展示歌词（`alx lyric`）或进行离线下载时，系统优先在此表查询记录，实现零网络开销、零 JS 沙箱启动的微秒级本地直达渲染：

```sql
CREATE TABLE IF NOT EXISTS lyrics_cache (
    song_id     TEXT NOT NULL,          -- 关联的平台歌曲 ID (与 search_cache.song_id 对齐)
    source      TEXT NOT NULL,          -- 音频物理来源平台
    lyric       TEXT NOT NULL,          -- 主同步 LRC 歌词文本
    tlyric      TEXT,                   -- 翻译歌词文本 (可为空)
    rlyric      TEXT,                   -- 罗马音歌词文本 (可为空)
    lxlyric     TEXT,                   -- 逐字 (动感) 歌词文本 (可为空)
    cached_at   TEXT NOT NULL,          -- 缓存写入的时间戳 (ISO 8601)
    PRIMARY KEY(song_id, source)        -- 使用复合主键约束，保证唯一性
);
```

### 1.4 `playlists` —— 用户自建歌单表
用户在本地所整理和新建的离线歌单：

```sql
CREATE TABLE IF NOT EXISTS playlists (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,   -- 歌单名称，保持全局唯一
    description TEXT,                   -- 用户自定义的歌单介绍描述
    song_count  INTEGER NOT NULL DEFAULT 0, -- 歌单包含的歌曲物理总量
    created_at  TEXT NOT NULL,          -- 建立时间 (ISO 8601)
    updated_at  TEXT NOT NULL           -- 最后一次更新时间
);

-- 特殊保留字系统预置歌单：
-- 1. "favorites" — 此歌单名为系统保留名称，用于托管用户通过 `alx fav` 锁定的“我的收藏”歌单。
```

### 1.5 `playlist_songs` —— 歌单内歌曲行项目表
该表保存自建歌单中所包含的具体歌曲映射行明细，采用外键强依赖 `playlists.id` 进行级联关系维护：

```sql
CREATE TABLE IF NOT EXISTS playlist_songs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE, -- 级联删除外键
    song_id     TEXT NOT NULL,          -- 关联平台内真实的歌曲 ID
    source      TEXT NOT NULL,          -- 物理音源平台
    name        TEXT NOT NULL,          -- 歌名缓存 (防止外键脱钩后无法展示)
    singer      TEXT NOT NULL,          -- 歌手缓存
    album_name  TEXT,                   -- 专辑名称
    interval    TEXT,                   -- 播放时长格式化表示
    pic_url     TEXT,                   -- 专辑封面外链
    position    INTEGER NOT NULL,       -- 歌曲在歌单内的物理展示与播放顺序排位索引
    added_at    TEXT NOT NULL           -- 歌曲添加入当前歌单的时间戳
);

-- 建立复合索引，保障在大歌单 (包含成千上万首歌曲) 被灌入播放队列时，实现微秒级的极速顺序提取
CREATE INDEX IF NOT EXISTS idx_playlist_songs_playlist ON playlist_songs(playlist_id, position);
```

### 1.6 `play_history` —— 用户播放历史记录表
该表保存用户的本地播放时间轴，供用户随时回溯和分析：

```sql
CREATE TABLE IF NOT EXISTS play_history (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    song_id     TEXT NOT NULL,
    source      TEXT NOT NULL,
    name        TEXT NOT NULL,
    singer      TEXT NOT NULL,
    album_name  TEXT,
    interval    TEXT,
    pic_url     TEXT,
    duration_played INTEGER,            -- 用户该次听歌实际听的有效时长值 (单位：秒)
    played_at   TEXT NOT NULL           -- 播放开始的时间 (ISO 8601 格式，降序检索)
);

-- 建立播放历史时间轴降序物理索引
CREATE INDEX IF NOT EXISTS idx_play_history_played_at ON play_history(played_at DESC);
```

### 1.7 `downloads` —— 离线下载持久化行表
用于记录并维护所有已完成、进行中或因网络问题损坏的音频下载任务落地状态：

```sql
CREATE TABLE IF NOT EXISTS downloads (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    song_id     TEXT NOT NULL,
    source      TEXT NOT NULL,
    name        TEXT NOT NULL,
    singer      TEXT NOT NULL,
    quality     TEXT NOT NULL,          -- 下载时的目标音质 (如 "flac")
    file_path   TEXT NOT NULL,          -- 落地音频的本地物理绝对文件路径
    file_size   INTEGER,                -- 最终音频文件的字节大小 (Bytes)
    status      TEXT NOT NULL,          -- 任务状态: "completed" (已完成), "failed" (失败), "partial" (分片传输中)
    downloaded_at TEXT NOT NULL,
    UNIQUE(song_id, source, quality)    -- 复合唯一性约束，防止同一音质音频行任务重复写入
);
```

---

## 2. 核心数据复合唯一键契约 (Composite Key)

在整个项目 `alx` (不管是底层的 Core 核心还是上层的 CLI 逻辑) 内部，**任何一首歌曲在物理平台实体上的绝对唯一性，恒等于以下复合元组的强绑定**：

$$\text{Song Entity Key} \equiv (\text{song\_id}, \text{source})$$

其中 `source` 是代表具体音乐物理源平台（如 `platformA`, `platformB` 等）的平台标识符，或者是对于通过自定义脚本解析的第三方源而言的具体音源脚本 `id`。

---

## 3. 本地数据库自动清洁维护策略 (Cleanup Policies)

为了保证 SQLite 在高频、高强度使用数年后，数据库体积依然完美控制在几兆字节的极致精简水准，我们设计了极轻量的**开机数据库自维护挂钩策略**：

| 目标数据表 | 触发频率 | 自动清洁自维护规则 (Cleanup Rule) |
|-----------|---------|--------------------------------|
| `search_cache` | 每次软件启动时 | 自动通过 `cached_at` 检索并彻底删除所有写入时间**超过 24 小时**的临时搜索缓存行。 |
| `play_history` | 每次软件启动时 | 自动通过 `played_at` 检索，只保留最近 `history.max_age_days`（默认 90 天，在配置文件可设置）的历史记录，其余自动彻底 Purge 擦除。 |
| `downloads`    | 每次软件启动时 | 自动查询并彻底擦除所有物理任务状态为 `status="partial"`（由于中途终止或损坏引起）且持续**超过 7 天**的坏死行任务。 |

---

## 4. 数据库 Schema 平滑升级演进策略 (Migration)

当项目迭代出全新功能需要对数据库扩展行、索引或新建表时，软件通过内置的 `schema_version` 版本标志机制实现优雅、无感的全自动运行时平滑升级：

```sql
-- 数据库内置版本特征表
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);
```

### 升级执行链：
1.  在 `alx` 启动时，第一步打开 SQLite 物理链接并查询 `schema_version` 中的当前 version 数值。
2.  自动从 `crates/lux-cli/src/library/migrations/` 内置的二进制中，提取所有大于当前版本号的 `.sql` 增量迁移脚本。
3.  以标准事务级 `Transaction` 形式，顺序串行跑完诸如 `001_init.sql`、`002_add_lyrics_cache.sql` 等升级事务。
4.  完全执行成功后，将最新的版本号标志回写写入 `schema_version` 表，完成安全、平滑升级！
