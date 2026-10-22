# flat-snowluma-mcp

本项目将 SnowLuma 官方 MCP 的 deferred 风格工具调用**展开为数百个扁平工具：每个 action
对应一个可直接调用、有参数类型的同名工具**。工具搜索、按需加载和调用编排交给 harness，
模型无需再经过官方 MCP 的 query/invoke 层来发现和调用 action。

这种组织方式面向对大量 MCP 工具有专门支持的 harness，例如 Pi 的 deferred 工具加载或
codemode，也适合通过 MCPHub 等专业 MCP 聚合网关使用。在这些环境下，扁平工具可以直接参与
工具检索和调用编排，减少额外的发现与调用步骤，提高使用效率。**使用前提是 harness 或网关
能够妥善处理大量工具**；直接将完整工具列表放入模型上下文会增加上下文开销。

本项目只保留与 SnowLuma OneBot API 的交互及上传分块、下载落盘等执行层封装，
不提供 SnowLuma 文档查询等功能。它适合让 agent 控制 QQ；开发 SnowLuma 周边生态、
查询文档和探索接口时，应使用官方 MCP。

实现使用 Rust 和 rmcp SDK，作为独立 stdio 进程运行：

```text
官方文档 MCP → 显式生成并固定版本 → 内嵌 action 目录
                                          ↓
MCP 客户端 → flat-snowluma-mcp（stdio）→ SnowLuma OneBot HTTP 端点
```

当前目录包含 200 个官方 action、443 个参数、6 个别名和 5 个 stream action，工具名与官方 action 一一对应。
覆盖范围为官方文档目录中的 OneBot action，不包含事件订阅或未列出的管理接口；目录收录不代表服务端已实现全部功能。
本项目将官方 MCP 的 action 展开为扁平工具，并实现对应的 MCP 主机执行层：上传分块、下载落盘和流式响应封装均由服务器处理。普通构建使用已提交的快照，不访问文档服务器。

## 目录快照对齐

| 项 | 值 |
| --- | --- |
| 对齐日期 | 2026-10-22 |
| SnowLuma 版本 | `@snowluma/mcp` 2.1.1 |
| SnowLuma commit | [`f8cb15c`](https://github.com/SnowLuma/SnowLuma/commit/f8cb15c27bca3d2d1bd044a921e55bfc44bb5e60) |
| 目录内容 hash | `0d7b4a9d6d173363ea139f09dcc67216fa912564662d4f1f6601449c66bc65db` |

快照由 `scripts/generate-catalog.mjs` 从官方文档 MCP 生成并已提交，运行时不再查询该服务器。

## 目录

```
src/catalog.rs    # 解析内嵌的 action 目录快照
src/schema.rs     # 原样复用官方输入 schema
src/patches.rs    # 官方描述选择与 4 项缺失描述的自写补充
src/names.rs      # 保留官方名称，检查客户端归一化后的冲突
src/validate.rs   # 跨字段约束、未知字段提示
src/onebot.rs     # OneBot v11 HTTP 传输（超时、取消、原始响应）
src/stream/       # Stream Action 帧解析、下载、分块上传
src/tools.rs      # 工具声明与调用分发
src/server.rs     # MCP stdio 服务器
assets/catalog.json         # 编译时嵌入的官方目录快照
docs/tool-audit.md           # 生成的工具来源审计报告
docs/tool-audit.json         # 完整工具声明与逐参数来源
scripts/generate-catalog.mjs # 从官方文档 MCP 重新生成快照
scripts/audit-tools.mjs      # 核对二进制声明并生成工具来源报告
scripts/check-execution-parity.mjs # 与官方 MCP 对比本地模拟场景
scripts/live-check.mjs       # 通过真实二进制探测指定 SnowLuma 实例
```

## 构建

```bash
cargo build --release
```

产物为单个二进制 `target/release/flat-snowluma-mcp`，通过 stdio 提供 MCP。

## 配置

地址与鉴权由环境变量提供（变量名与官方 `@snowluma/mcp` 一致，便于复用现有连接配置）：

| 变量 | 含义 |
| --- | --- |
| `SNOWLUMA_MCP_ENDPOINT` | OneBot HTTP 端点，例如 `https://snowluma.example.com/a/`。缺省时只提供声明，不执行。 |
| `SNOWLUMA_MCP_TOKEN` | 访问令牌（`Authorization: Bearer`）。 |
| `SNOWLUMA_MCP_TIMEOUT_MS` | 普通请求超时；Stream 连接与逐帧空闲超时。默认 `30000`。 |
| `SNOWLUMA_MCP_STREAM_DIR` | 下载落盘的 MCP 服务器主机目录。默认系统临时目录下的 `flat-snowluma-downloads`。 |
| `SNOWLUMA_MCP_UPLOAD_ROOT` | 允许自动上传读取的既有目录；未设置时不能使用 `execution.input_file`，手动分块仍可用。 |
| `SNOWLUMA_MCP_MAX_STREAM_BYTES` | 单文件下载/上传上限，默认 4 GiB。 |
| `FLAT_SNOWLUMA_LOG` | 日志过滤（`tracing` EnvFilter），日志写 stderr。 |

使用 `mcpServers` 格式的客户端配置示例（具体配置位置和环境变量语法以客户端为准）：

```json
{
  "mcpServers": {
    "snowluma": {
      "command": "/path/to/flat-snowluma-mcp",
      "env": {
        "SNOWLUMA_MCP_ENDPOINT": "https://snowluma.example.com/a/",
        "SNOWLUMA_MCP_TOKEN": "YOUR_SNOWLUMA_TOKEN",
        "SNOWLUMA_MCP_UPLOAD_ROOT": "/tmp/snowluma-share"
      }
    }
  }
}
```

创建上传目录并替换示例端点和令牌后再运行。服务器可用于支持 MCP stdio 的客户端。

## 工具与文本来源

工具通过标准 MCP `tools/list`、`tools/call` 提供，工具搜索和按需加载由客户端实现。
官方名称、参数、schema 和描述保存在 [assets/catalog.json](assets/catalog.json)，来源版本与提交见上表。
188 个 action 直接使用官方摘要；8 个缺少摘要的 action 复用官方返回说明或返回字段描述。
其余 4 个 action 没有可复用的描述，由本项目补充：

| 工具 | 自写描述 |
| --- | --- |
| `cancel_group_album_media_like` | 取消群相册媒体点赞 |
| `do_group_album_comment` | 评论群相册媒体 |
| `set_group_album_media_like` | 点赞群相册媒体 |
| `upload_image_to_qun_album` | 上传图片到群相册 |

## 调用与结果

- 每个 action 一个同名工具（包括 `.get_word_slices`），参数平铺，
  保留原名、必填、枚举、默认值、单位和边界。
- 描述直接使用官方原文；官方缺少摘要时优先复用其他官方描述，仅在没有可复用表述时自写补充。
  官方约束与别名原文追加，不添加搜索同义词；action 参数字段 schema 与官方快照完全一致。
  `upload_file_stream` 另包含官方 MCP 的 `execution` 选项，并调整自动上传的必填条件。
- 跨字段约束（`exactly one of`、`at least one of`、`X required when Y`、取值集合）在调用前校验，
  不只写在描述里。
- 未知字段返回合法字段列表，避免 `message_count` 被静默忽略。
- 请求不自动重放，避免客户端不知道的额外调用。
- 普通 HTTP 响应保留完整 OneBot 封套，包括 `status`、`retcode`、`data`、`wording` 等。
  和官方 MCP 一样，业务失败仍作为数据返回；传输、协议或参数错误设置 `isError: true`。
  调用方先检查 `isError`，再检查普通响应的 `status` / `retcode`。`structuredContent` 与 `content` 的 JSON 负载相同。
- Stream 文件下载返回 `file_path`、`file_size`、`sha256`、`frame_count` 和完整 `terminal`。
  通用流返回 `frame_count`、`frames` 和 `terminal`；不再添加自定义 `kind` / `frames_file` 格式。
- 全部目录另作为资源 `snowluma://onebot/actions` 暴露。

官方 `returnsSchema` 描述 OneBot 响应中的 `data`；本项目原样将其用于成功响应的 `outputSchema` `data` 字段。
外层条件封套由本项目定义，允许业务失败保留原始数据而不被成功 schema 拒绝。
工具注解的只读性采用官方 `readOnly`，不按工具名前缀推断；写操作标注为可能有破坏性。
别名用于目录查找和工具描述，不另行注册同一工具。列表沿用上游分页，服务器不静默裁剪结果。

以下为 `tools/call` 的参数示例，群号和路径均需替换为目标实例的数据：

```json
{
  "name": "get_group_msg_history",
  "arguments": {"group_id": 123456, "count": 3}
}
```

发送图文可直接传消息段数组；图片路径必须由机器人侧访问：

```json
{
  "name": "send_group_msg",
  "arguments": {
    "group_id": 123456,
    "message": [
      {"type": "image", "data": {"file": "/tmp/onebot-stream/upload/example.jpg"}},
      {"type": "text", "data": {"text": "处理结果"}}
    ]
  }
}
```

## MCP 主机执行层

所有工具均对应官方 action，不另行注册 `upload_local_file` 或 `download_to_file`。
本地文件自动上传直接使用 `upload_file_stream`：

```json
{
  "name": "upload_file_stream",
  "arguments": {
    "file_retention": 300000,
    "execution": {"input_file": "/tmp/snowluma-share/chart.png"}
  }
}
```

`execution` 的名称、结构和描述直接取自官方 MCP `invoke_action`，保存在快照的 `execution` 部分。
`input_file` 是 MCP 主机文件路径，必须在 `SNOWLUMA_MCP_UPLOAD_ROOT` 内；`execution` 不会作为 OneBot 参数发送。
MCP 以 512 KiB 分块读取文件，生成 UUID、Base64、分块序号、合并请求和 SHA-256 校验，并返回
`file_path`、`file_size`、`sha256`、`chunk_count`、`stream_id` 和 `terminal`。
返回路径位于 SnowLuma 机器人主机，可供后续发送文件接口使用。

自动上传时不要同时传 `stream_id`、`chunk_data`、`chunk_index`、`total_chunks`、`file_size`、
`expected_sha256`、`is_complete`、`reset` 或 `verify_only`，这些字段由 MCP 管理；冲突会报错。
不提供 `execution.input_file` 时仍可按官方参数手动分块，必须提供 `stream_id`。
因此上传工具的字段保持官方原文，但外层必填条件由本项目适配为手动或自动上传二选一。

下载直接调用 `download_file_stream`、`download_file_image_stream` 或 `download_file_record_stream`：

```json
{
  "name": "download_file_stream",
  "arguments": {"file": "https://example.com/report.pdf"}
}
```

服务器自动消费所有分块、校验索引和大小，计算 SHA-256，并等待合法终止帧和流结束后才提交文件。
文件先写入权限受限的 `.part`；取消、超时、协议错误或大小不符时删除未完成文件。
下载路径位于 MCP 主机，不是机器人主机；客户端需要具备访问该主机文件的能力才能读取。

执行限制与官方 MCP 2.1.1 对齐：

- 最多同时执行 4 个顶层 Stream Action；自动上传的内部请求共用一个名额。
- 每文件上限由 `SNOWLUMA_MCP_MAX_STREAM_BYTES` 指定；下载目录配额为其两倍，已完成文件也计入配额，需由使用者移动或删除。
- 通用流最多 256 帧、256 KiB 原始帧文本；单帧最多 24 MiB，文件最多 200,000 个分块。超限返回错误。
- Stream 超时分别作用于建立响应和每次读取的空闲等待，不限制持续有数据的传输总时长。
- 自动上传失败后使用独立的 5 秒期限尝试重置远端流，包括调用方已经取消的情况；重置不能撤销已提交文件，默认保留期为 300,000 ms。

本项目保留通过端点启用执行的现有配置方式；上述对齐范围是传输和文件执行行为，不引入官方通用入口的 docs/read/write 工具路由。

## 重新生成目录快照

```bash
node scripts/generate-catalog.mjs                    # 默认固定 2.1.1
SNOWLUMA_MCP_VERSION=latest node scripts/generate-catalog.mjs # 显式升级到最新发布版
```

脚本通过 stdio 驱动官方文档 MCP（`initialize` → `list_actions` → 逐项 `get_action` →
`list_categories`），另通过 `tools/list` 获取官方 MCP 的执行选项 schema，写入 `assets/catalog.json` 并记录来源版本、发布提交、抓取时间与内容 hash。
升级时比较 action、参数与约束变化即可。运行时不再查询该服务器。

## 工具来源审计

`scripts/audit-tools.mjs` 使用 Node.js 内置模块，将编译后的 MCP 声明与 `assets/catalog.json` 核对，
检查工具与参数名称、官方描述原文、输入 schema、只读注解和返回 schema。上传执行选项与 action 参数分别标记来源，并明确记录必填条件的扁平化适配。
脚本还核对二进制暴露的目录版本和 hash，避免使用旧构建产物。

```bash
cargo build --release
node scripts/audit-tools.mjs --binary target/release/flat-snowluma-mcp
```

默认生成 [来源报告](docs/tool-audit.md) 和 [JSON 明细](docs/tool-audit.json)，包含每个工具的完整声明、
逐参数 schema、描述原文和来源 JSON Pointer，并标记本项目自写内容。可通过 `--output-dir` 指定报告目录。
审计不评判自写文本的业务语义；官方表述或声明不一致时返回非零退出码。

脚本仅执行 MCP 初始化、`tools/list` 与目录 `resources/read`，不调用业务工具。
它会移除子进程中的 SnowLuma 连接配置，使用独立临时目录并在结束后清理，无需真实实例或凭据。

## 测试

```bash
cargo test          # 单元测试 + 针对进程内模拟 OneBot 服务的集成测试
cargo clippy --all-targets
```

集成测试覆盖发现、普通调用、逻辑失败、未知字段、文件下载提交与截断清理、协议错误清理、
通用帧汇总、通用流错误帧、流式限额、分块上传与 SHA-256 校验、上传错误帧、上传路径围栏，以及超时、取消和目录配额。
写操作用模拟服务验证，不触达真实 QQ。模拟服务的应答形状（普通回复不加帧分隔符、stream 帧带
`stream`/`message`/`wording`、上传完成用 `data.status: "file_complete"`、`file_info` 的
`chunk_size` 只是提示）与运行中的真实实例一致。

### 与官方 MCP 比较执行结果

```bash
node scripts/check-execution-parity.mjs --binary target/release/flat-snowluma-mcp
```

脚本通过 npx 启动快照固定版本的官方 MCP，同时启动本项目二进制，让两者访问同一个本地模拟 HTTP 服务。
它比较普通响应、三种文件下载、自动上传、空文件、手动分块、重置、异常流和超时等场景的结果。
随机文件路径和流 ID 会归一化，下载内容与上传 SHA-256 则直接校验。错误场景比较是否失败，不要求错误文案相同。
可用 `--official-server /path/to/dist/server.js` 指定已安装的同版本官方 MCP，避免 npx 下载。
该测试无需真实 SnowLuma 实例或 QQ 凭据，也不会执行真实发送操作。

### 真实实例探测

`scripts/live-check.mjs` 是需显式执行的人工探测脚本，使用 Node.js 启动指定二进制，通过
stdin/stdout 的逐行 JSON-RPC 执行 MCP `initialize`、`notifications/initialized`、`tools/list` 和 `tools/call`。
随后读取群历史、发送文本、上传并发送文件，以及尝试下载近期图片。
它覆盖从二进制进程、MCP stdio 到真实 OneBot HTTP 服务的整条调用链。

它需要外部实例、群号和可能的令牌，会产生真实写操作；当前主要打印结果，不保证失败时返回非零退出码，
近期历史没有图片时下载步骤也可能失败。因此它不参与默认 `cargo test`，也不适合作为 CI 成功与否的判据。
自动化测试使用 `tests/integration.rs` 中可重复、无需凭据的模拟服务。

```bash
cargo build --release
```

然后手动执行：

```bash
SNOWLUMA_MCP_ENDPOINT=https://snowluma.example.com/a/ \
  SNOWLUMA_MCP_TOKEN=YOUR_SNOWLUMA_TOKEN SNOWLUMA_TEST_GROUP=123456 \
  node scripts/live-check.mjs target/release/flat-snowluma-mcp
```

端点与群号必须显式设置；`123456` 为示例群号。该脚本会向指定群真实发送消息与上传文件。
