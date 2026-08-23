# JS 音源沙箱桥接 API 规范

`alx` 支持载入符合开放通用音源规范的自定义 JavaScript 音源脚本。宿主程序内部通过内置的轻量 QuickJS 解析引擎，在安全隔离的沙箱环境内运行脚本，并通过 `globalThis.lx` 对象向脚本注入标准的底层系统接口调用与回调契约。

---

## 1. 宿主注入 API 详解 (`globalThis.lx`)

### 1.1 核心方法 (Core Methods)

#### `lx.request(url, options, callback)`

发起跨域异步网络请求。该方法在接口规范上与标准通用客户端网络库完全等价。

```typescript
lx.request(
  url: string,
  options: {
    method?: "GET" | "POST" | "PUT" | "DELETE"
    headers?: Record<string, string>
    body?: string | Uint8Array
    form?: Record<string, string>
    formData?: Record<string, any>
    timeout?: number          // 超时毫秒数，最大 60000 (1分钟)
    follow_max?: number       // 最大重定向追踪深度
  },
  callback: (err: Error | null, response: {
    statusCode: number
    statusMessage: string
    headers: Record<string, string>
    body: any                 // 宿主会自动尝试解析 JSON，成功则为 Object，否则为 raw
    raw: Uint8Array           // 响应的原始二进制数据流 Buffer
  }, body: any) => void
): () => void  // 执行该返回函数可立即强制中止网络请求 (Abort)
```

**Rust 引擎桥接层实现说明：**

* 底层完全通过 Rust 高性能网络库 `reqwest` 实现网络 IO，并严格在沙箱线程隔离运行。
* 支持在主机 `alx` 配置文件中读取全局 `network.proxy` 代理，并自动透明地应用到请求连接池。

#### `lx.on(eventName, handler)`

注册音源事件回调函数。目前只支持且必须注册 `'request'` 监听器。

```javascript
lx.on('request', async ({ action, source, info }) => {
  // 处理来自 alx 宿主程序发起的数据解析请求
  switch (action) {
    case 'musicUrl': return await getMusicUrl(source, info) // 解析音频直链
    case 'lyric':    return await getLyric(source, info)    // 解析同步歌词
    case 'pic':      return await getPic(source, info)      // 解析专辑封面图
  }
})
```

#### `lx.send(eventName, data)`

向宿主发送初始化或版本告警等生命周期事件。该方法返回一个 `Promise`。

```javascript
// 在脚本初始化完毕后，必须发起这一事件以成功向宿主注册自身支持的音质与平台
lx.send('inited', {
  status: true,
  openDevTools: false,
  sources: {
    platformA: { type: 'music', actions: ['musicUrl'], qualitys: ['128k', '320k', 'flac'] },
    platformB: { type: 'music', actions: ['musicUrl', 'lyric'], qualitys: ['128k', '320k', 'flac', 'flac24bit'] },
  }
})
```

### 1.2 辅助工具集 (Utility Methods)

#### `lx.utils.crypto`

提供加密计算相关的工具，常用于各大音乐平台接口的参数加解密混淆（如网易云的 weapi/eapi 等）：

```javascript
lx.utils.crypto.md5(string)                          // 计算 MD5 摘要，返回十六进制 Hex 字符串
lx.utils.crypto.aesEncrypt(buffer, mode, key, iv)    // AES 加密，返回 Buffer 实例
lx.utils.crypto.rsaEncrypt(buffer, key)              // RSA 加密，返回 Buffer 实例
lx.utils.crypto.randomBytes(size)                    // 获取指定长度的强随机数 Buffer 实例
```

* `mode` 采用标准 OpenSSL 命名规范，如 `"aes-128-ecb"`, `"aes-256-cbc"`, `"aes-128-obc"` 等。

#### `lx.utils.buffer`

二进制 Buffer 构建：

```javascript
lx.utils.buffer.from(data, encoding?)    // 构建 Buffer 实例
lx.utils.buffer.bufToString(buf, encoding) // 将二进制 Buffer 还原为特定编码的字符串
```

* `encoding` 支持的字符编码：`"utf8"`, `"hex"`, `"base64"`, `"binary"`。

#### `lx.utils.zlib`

数据压缩解压缩工具：

```javascript
lx.utils.zlib.inflate(buffer)     // Zlib 解压缩 -> Promise<Buffer>
lx.utils.zlib.deflate(buffer)     // Zlib 压缩 -> Promise<Buffer>
```

### 1.3 核心元数据读取

```javascript
lx.env                // 环境标识，在 alx 中恒等于 "cli"
lx.version            // 音源桥接协议规范版本，恒等于 "2.0.0"
lx.currentScriptInfo  // 获取当前正在执行的音源脚本的头信息，包含 name, version, author, homepage 等
lx.EVENT_NAMES        // 注册事件字典常量：{ request: "request", inited: "inited" }
```

---

## 2. 音源脚本编写契约

一个被 `alx` 正常接受的音源脚本必须同时满足以下两个要件：

1. 在全局执行时，必须同步向宿主发送 `lx.send('inited', ...)` 事件以完成设备注册。
2. 必须通过 `lx.on('request', handler)` 注册全局事件拦截器，并对 `action` 为 `"musicUrl"`、`"lyric"`、`"pic"` 的事件进行精准的 Promise 回调处理。

### 2.1 请求拦截详细规格 (Request Spec)

#### 动作类型一：`musicUrl` (解析音频直链)

* **输入参数形式 (`info`)**：

    ```javascript
    info = {
      type: "320k",           // 宿主请求解析的目标音质
      musicInfo: {
        songmid: "12345",     // 歌曲在音乐平台的唯一 ID
        name: "晴天",
        singer: "周杰伦",
        // ... 其他由搜索缓存传递的特定音乐详情
      }
    }
    ```

* **期望的返回值**：必须返回一个 `String` 类型的音频播放直链 URL，且该直链应能在无 Cookie 或带有音源所定义 Header 的情况下被 `mpv` 解码器成功读取。

#### 动作类型二：`lyric` (解析歌词)

* **输入参数形式 (`info`)**：

    ```javascript
    info = {
      musicInfo: { songmid: "12345", ... }
    }
    ```

* **期望的返回值**：必须返回一个包含歌词各轨道字符串的 Object 格式：

    ```javascript
    return {
      lyric: "[00:00.00] 晴天\n[00:05.00] 故事的小黄花...", // 主歌词轨道 (LRC)
      tlyric: "[00:00.00] Sunny day...",                  // 翻译歌词轨道 (可选)
      rlyric: null,                                       // 罗马音歌词轨道 (可选)
      lxlyric: null                                       // 逐字歌词轨道 (可选)
    }
    ```

#### 动作类型三：`pic` (解析专辑封面图)

* **期望的返回值**：必须返回一个图片直链 `String`。

### 2.2 脚本元数据头规范 (Header Comment)

宿主在加载 JS 文件时，通过扫描文件的首个注释块提取该音源的显示基本信息：

```javascript
/*!
 * @name 自定义演示音源
 * @description v1.0.0 - 探索 CLI 音乐播放可能性
 * @version v1.0.0
 * @author 作者名称
 * @homepage www.example.com
 */
```

---

## 3. 标准音源脚本中文编写模板

```javascript
/*!
 * @name 演示音源模板
 * @description v1.0.0 - 标准中文化的 JS 桥接脚本模板
 * @version v1.0.0
 * @author 开发者
 */

const {
  EVENT_NAMES, request, on, send, utils, env, version
} = globalThis.lx;

// 封装原生 request 为优雅的 Promise
const httpFetch = (url, options = { method: "GET" }) => {
  return new Promise((resolve, reject) => {
    request(url, options, (err, resp) => {
      if (err) return reject(err);
      resolve(resp);
    });
  });
};

// 实际解析音频播放 URL 的函数
const handleGetMusicUrl = async (source, musicInfo, quality) => {
  const songId = musicInfo.songmid;
  const targetUrl = `https://api.my-music-server.com/play?id=${songId}&quality=${quality}`;
  
  const resp = await httpFetch(targetUrl, {
    method: "GET",
    headers: {
      "User-Agent": `lx-music-${env}/${version}`,
    }
  });
  
  const { body } = resp;
  if (body && body.code === 200 && body.url) {
    return body.url; // 成功返回 playable URL
  }
  throw new Error("解析音频失败，可能由于服务暂时不可用或歌曲不存在");
};

// 监听宿主指令
on(EVENT_NAMES.request, async ({ action, source, info }) => {
  switch (action) {
    case "musicUrl":
      return await handleGetMusicUrl(source, info.musicInfo, info.type);
    default:
      throw new Error(`当前音源不支持 action: ${action}`);
  }
});

// 向宿主注册我们的音源
send(EVENT_NAMES.inited, {
  status: true,
  openDevTools: false,
  sources: {
    platformA: {
      name: "演示平台A",
      type: "music",
      actions: ["musicUrl"],
      qualitys: ["128k", "320k", "flac"]
    }
  }
});
```
