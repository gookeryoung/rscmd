# ADR 0001：配置、状态与缓存目录（`RSCMD_HOME`）

- 状态：提议（待批准后实现）
- 日期：2026-11-16
- 关联：`docs/infrastructure-plan.md` §2.5、P2-3（下载缓存）、P2-5（目录 resolver）

## 背景

仓库当前没有任何 config/state/cache 目录约定，`ToolCtx` 也不携带路径信息。
基础设施计划要求：在首个持久化或缓存需求出现前先定目录约定，避免多个工具各自
发明路径、各自处理写入失败。P2-3 的网络层已具备重试与统一错误码，下载缓存是
第一个候选消费者；P2-5 的 resolver 尚无代码。

本 ADR 只定义目录与优先级、失败语义和缓存文件格式，不新增工具行为。

## 决策

### 1. 单一根目录

所有持久化内容都落在同一个根目录下，由以下优先级决定（从高到低）：

1. 未来的显式 CLI 参数（如 `--home <dir>`，本次不实现）；
2. 环境变量 `RSCMD_HOME`；
3. 平台默认：
   - Windows：`%LOCALAPPDATA%\rscmd`
   - macOS：`~/Library/Application Support/rscmd`
   - Linux/其他 Unix：`$XDG_DATA_HOME/rscmd`，未设置时 `~/.local/share/rscmd`
4. 无法确定用户主目录时：不猜测，返回目录不可用错误。

根目录只由 resolver 计算，工具不得自行读取上述环境变量或拼接路径。

### 2. 布局

```
<RSCMD_HOME>/
  cache/
    downloads/        # 下载缓存（按内容寻址）
  state/              # 需要跨次保留的工具状态
  logs/               # 可选：日志文件（当前无 --log-file，先占位不创建）
```

- 目录按需创建，读取路径本身不产生副作用；
- 首次写入前创建父目录；创建或写入失败必须返回明确错误码（见第 4 节）；
- 不建立版本号或迁移机制：目录内文件自带 `schema` 字段，需要演进时按文件升级。

### 3. 下载缓存格式

- 键：规范化 URL（去 fragment、scheme/host 小写）的 SHA-256，十六进制；
- 每条缓存两项：
  - `<digest>.bin`：原始响应体；
  - `<digest>.json`：`{ "schema": "rscmd-cache/1", "url", "size", "sha256", "etag", "fetched_at" }`；
- 写入使用临时文件 + 同名 rename，避免半写文件被当成命中；
- 命中判定：`.bin` 与 `.json` 均存在、`size` 一致；工具声明期望校验和时再比对 `sha256`；
  任一项不符视为未命中并重新下载；
- 默认不启用：只有显式选择缓存的工具才读写 `cache/downloads/`，现有工具行为不变；
- 不做自动淘汰；本 ADR 不引入清理命令，磁盘占用由用户手动管理（后续需要时另立 ADR）。

### 4. 失败语义与错误码

新增并登记到 `ERROR_CODE_KINDS`：

| 错误码 | kind | 场景 |
| --- | --- | --- |
| `RSCMD_HOME_UNAVAILABLE` | runtime | 无法确定根目录（无 `RSCMD_HOME` 且无平台目录与主目录） |
| `STATE_DIR_UNAVAILABLE` | runtime | 目录已确定但无法创建或不可写 |
| `STATE_WRITE_FAILED` | runtime | 创建目录/写文件/rename 失败 |
| `CACHE_CORRUPT` | runtime | 缓存元数据无法解析或与内容不一致，且调用方要求严格校验 |

失败不静默降级到内存缓存：调用方要么拿到明确错误，要么显式选择「不使用缓存」。

### 5. resolver API（实现时）

- `home() -> Result<PathBuf, DirError>`
- `cache_dir(kind) -> Result<PathBuf, DirError>`（`kind` 目前只有 `downloads`）
- `state_dir() -> Result<PathBuf, DirError>`
- `logs_dir() -> Result<PathBuf, DirError>`
- 读取接口不创建目录；写路径由调用方显式调用 `ensure(path)` 或写入时自动创建。
- resolver 放在 `rscmd-runtime`，不进入 `rscmd-core`，避免核心层依赖文件系统布局。

## 备选方案

1. **XDG 全套（config/data/state/cache 分离）**：跨平台映射复杂，当前只有三类内容，
   统一根目录更易解释；未来需要时可用 `RSCMD_CONFIG_HOME` 等叠加。
2. **每个工具自管目录**：重复实现优先级与错误处理，且无法统一清理，否决。
3. **使用第三方目录库（`directories`/`dirs`）**：会新增依赖，且规则与上面的优先级不完全一致；
   本仓库规模下二十行逻辑即可覆盖，按 D1 不引入。

## 影响

- 纯新增：不改变现有 JSON 契约、CLI 参数与发布物；
- 第一个消费者是下载缓存（P2-3 剩余部分），启用需工具显式选择；
- 目录不可用时返回明确错误码，不阻塞其它工具。

## 待批准后实施的顺序

1. `rscmd-runtime` 目录 resolver + 单元测试（优先级、平台分支、不可写错误）；
2. 错误码登记与 doctor 可选展示（仅当目录已存在时）；
3. 下载缓存读写 + 命中/损坏/并发写测试，接入 `download_with` 的可选参数；
4. CHANGELOG 与文档生成物同步。
