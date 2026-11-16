# ADR 0001：配置、状态与缓存目录（`RSCMD_HOME`）

- 状态：已批准（已落地）
- 日期：2026-11-16
- 关联：`docs/infrastructure-plan.md` §2.5、P2-3（下载缓存）、P2-5（目录 resolver）

## 背景

仓库当前没有任何 config/state/cache 目录约定，`ToolCtx` 也不携带路径信息。
基础设施计划要求：在首个持久化或缓存需求出现前先定目录约定，避免多个工具各自
发明路径、各自处理写入失败。P2-3 的网络层已具备重试与统一错误码，下载缓存是
第一个候选消费者；P2-5 的 resolver 尚无代码。

本 ADR 定义目录与优先级、失效与容量策略、失败语义和缓存文件格式，不新增工具行为。

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
  - `<digest>.json`：`{ "schema": "rscmd-cache/1", "url", "size", "sha256", "fetched_at" }`；
- 写入使用临时文件 + 同名 rename，避免半写文件被当成命中；临时文件由
  `fs::atomic_write` 生成，命名为 `.rscmd-tmp-<hex>` 后缀；
- 命中判定：`.bin` 与 `.json` 均存在、`size` 一致、`sha256` 与内容一致；
  任一项不符视为未命中并删除该条目后重新下载；
- **失效**：默认 TTL 7 天（按 `fetched_at` 判定，读取时惰性删除过期条目）；
  工具可用 `CachePolicy` 覆盖；
- **容量边界**：默认总大小 256 MiB、条目数 1024；每次写入后按 `fetched_at` 从旧到新
  淘汰，直到同时满足两个上限；临时文件（`.rscmd-tmp-*`）与孤立 `.bin` 一并清理；
- 未命中、损坏、过期、淘汰都只是缓存行为，不改变工具结果；
- 默认不启用：只有显式选择缓存的工具才读写 `cache/downloads/`，现有工具行为不变；
- 本 ADR 不引入独立的 `rscmd cache` 清理命令，容量策略在写入路径内自动执行。

### 4. 失败语义

- **下载缓存静默降级**：根目录无法解析、目录无法创建或不可写、rename/写文件失败时，
  缓存层视为「未启用」，直接走网络下载；不向工具或调用方报错，也不写 stderr。
- 缓存内容损坏（元数据无法解析、大小或哈希不符）按未命中处理并删除条目，不报错。
- `state/` 与 `logs/` 未来写入失败才映射为明确错误码（`STATE_DIR_UNAVAILABLE` /
  `STATE_WRITE_FAILED`，登记到 `ERROR_CODE_KINDS`）；当前没有消费者，不提前引入。
- resolver 仍返回 `Result`，便于未来 state/logs 消费者区分「未配置」与「不可写」。

### 5. resolver 与缓存 API

- `DirEnv { vars, platform, home_dir }`：环境变量、平台、主目录全部可注入；
  `DirEnv::from_process()` 读取真实环境。
- `Dirs::resolve(&DirEnv) -> Result<Dirs, DirError>`；`cache_dir()` / `state_dir()` /
  `logs_dir()` 只拼接路径，不创建目录。
- `DownloadCache::open(&Dirs, CachePolicy)`：目录不可创建时返回 `None`（静默降级）。
- `DownloadCache::with_clock(root, policy, clock)`：时钟可注入，测试过期与淘汰。
- `net::download_with_cache(url, timeout, RetryPolicy, Option<&DownloadCache>)`：
  命中直接返回；未命中走 `download_with` 并写回；`None` 或写入失败都按无缓存处理。
- 写入前 `create_dir_all`；创建失败直接放弃本次缓存写入。
- 跨进程互斥用 `lock::DirLock`：`<dir>/<name>.lock` 以 `create_new` 独占创建，
  `try_acquire` 立即返回 `LOCK_BUSY`，`acquire(dir, name, timeout)` 轮询到超时返回
  `LOCK_TIMEOUT`，目录只读或不可创建返回 `LOCK_UNAVAILABLE`（三个码均登记在
  `ERROR_CODE_KINDS`，kind 为 runtime）；持有者 Drop 时删除锁文件（正常返回与 panic 都释放，
  进程被强杀会残留，因此调用方必须带超时）。下载缓存写入同一键时用 500 ms 上限的
  `acquire` 串行化，拿不到锁就放弃本次写入，不改变工具结果。
- resolver 与缓存放在 `rscmd-runtime`，不进入 `rscmd-core`，避免核心层依赖文件系统布局。

### 6. 跨平台差异

- Windows：`%LOCALAPPDATA%\rscmd`；路径大小写不敏感，但缓存文件名统一用十六进制小写，
  不依赖真实大小写；`rename` 不能覆盖已有目标，刷新缓存时先删除旧文件再 rename。
- macOS：`~/Library/Application Support/rscmd`。
- Linux/其他 Unix：`$XDG_DATA_HOME/rscmd`，缺省 `~/.local/share/rscmd`。
- 任何平台都优先使用 `RSCMD_HOME`；空字符串视为未设置。

## 备选方案

1. **XDG 全套（config/data/state/cache 分离）**：跨平台映射复杂，当前只有三类内容，
   统一根目录更易解释；未来需要时可用 `RSCMD_CONFIG_HOME` 等叠加。
2. **每个工具自管目录**：重复实现优先级与错误处理，且无法统一清理，否决。
3. **使用第三方目录库（`directories`/`dirs`）**：会新增依赖，且规则与上面的优先级不完全一致；
   本仓库规模下二十行逻辑即可覆盖，按 D1 不引入。

## 影响

- 纯新增：不改变现有 JSON 契约、CLI 参数与发布物；
- 第一个消费者是下载缓存（P2-3 剩余部分），启用需工具显式选择；
- 缓存不可用时静默降级，不阻塞工具执行。

## 实施顺序

1. `rscmd-runtime` 目录 resolver + 单元测试（优先级、平台分支、空值/缺值）；
2. 下载缓存读写、TTL 失效、容量淘汰、损坏清理 + 可注入时钟/策略的单测；
3. `download_with_cache` 接入与端到端测试（命中不重复下载、不可写时降级）；
4. 跨进程目录锁 `lock::DirLock` + 8 进程竞争集成测试，接入缓存写入路径；
5. CHANGELOG 与文档生成物同步；缓存失败不上浮（仅锁的原语错误码供未来消费者使用）。

## 落地状态

五步均已实现并随 `main` 入库：

- `home::Dirs::resolve` / `cache_dir` / `state_dir` / `logs_dir`（`crates/rscmd-runtime/src/home.rs`）；
- `cache::DownloadCache`（TTL、容量淘汰、损坏清理，时钟可注入）；
- `net::download_with_cache`（命中不重复下载，未启用或不可写时静默降级）；
- `lock::DirLock`（`try_acquire` / `acquire`）并已接入缓存写入路径（500 ms 上限）；
- `ERROR_CODE_KINDS` 已登记 `LOCK_BUSY` / `LOCK_TIMEOUT` / `LOCK_UNAVAILABLE`。

## 交接

- 下一步：当前没有工具默认启用缓存；出现真实下载缓存需求时，工具侧传
  `Some(&DownloadCache)` 即可，不需要改 resolver。
- 已知坑：Windows 上 `rename` 不覆盖已有目标，刷新缓存走「先删旧文件再 rename」；
  进程被强杀会残留 `.lock`，调用方必须使用带超时的 `acquire`，不能依赖 `try_acquire` 死等。
- `logs/` 目录仍只拼接不创建：`--log-file` 在基础设施计划里属「有明确需求再启动」，当前无消费者。
