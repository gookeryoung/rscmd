# 更新日志

本文件按版本分段（标题形如 `## [0.1.0] - 2026-11-16`）。
发布前 `cargo xtask version-check` 会校验 `[workspace.package].version` 存在对应段落。

## [0.1.0] - 2026-11-16

### 新增

- 错误码到错误大类的中央登记表 `ERROR_CODE_KINDS`；`RscmdError::coded` 依表推导 `kind` 与退出码，
  未登记码统一回退 `invalid_input`（退出码 3），避免同一错误码在不同调用点给出不同分类。
- 新增全局 `--verbose`/`-v`：在 stderr 输出 `[rscmd] tool=<工具名> elapsed=<毫秒>ms` 执行跟踪，
  stdout 的人类可读文本与 JSON 信封保持原样。
- `rscmd schema` 的输出改为 `rscmd-schema/1` 自描述对象：`version`、`tools`、`builtins`、`global`。
  （契约变更：此前 `data` 直接是工具元数据数组。）
- 新增全局 `--timeout <SECONDS>`（0 表示不限制）：解析后写入 `ToolCtx.timeout`，工具内的
  子进程（`rscmd_runtime::proc`）与网络请求（`rscmd_runtime::net`）通过 `ToolCtx::timeout_or`
  统一取值；未设置时沿用工具自身的默认超时。
- Ctrl+C 现在以退出码 130 结束：CLI 注册 SIGINT 处理，按当前输出模式渲染 `kind=interrupted`
  的错误信封（`--json` 下为写 stderr 的 `rscmd-error/1`）。
- `ERROR_CODE_KINDS` 登记 `TIMEOUT`（runtime，退出码 1）与 `INTERRUPTED`（interrupted，退出码 130）。
- 新增内建命令 `rscmd completion <SHELL>`：按完整命令树生成 bash / zsh / fish / powershell 补全脚本，
  脚本原文走结果 `text` 字段（`--json` 下为 `rscmd-result/1` 信封）。
- 新增工具 `hash`（crypto 域，别名 `hashtool`）：计算文本或文件的 SHA-256，返回 `hex`/`bytes`/`source`；
  `TEXT` 与 `--file` 二选一，同时给出或都不给按 `INVALID_ARGS`（退出码 3）拒绝。
- `ERROR_CODE_KINDS` 登记 `IO`（runtime，退出码 1），用于文件读取失败等运行期 IO 错误。
- `rscmd doctor` 新增两项检查：`Python 扩展`（`_native` 产物是否旧于最新 Rust 源码，安装态或未构建时跳过）
  与 `终端编码`（控制台代码页不是 UTF-8 时给出 `chcp 65001` 建议），检查项由 3 项增至 5 项；
  环境结论可通过 `DoctorProbe` 注入，便于单测覆盖各分支。
- 结果信封新增可选 `subcommand` 字段：工具带下级子命令时写入子命令名；普通工具不出现该键。
  属兼容扩展（`rscmd-result/1` 不变），无子命令时消费方看到的字段集合与之前完全一致。
- 新增 `rscmd mcp`：以 MCP stdio server 模式运行，支持 `initialize`、`ping`、`tools/list`、
  `tools/call`。仅暴露有 JSON 入口的非 GUI 工具，`inputSchema` 直接复用工具 schema；
  协议版本 2025-06-18。未引入第三方依赖；Rust 二进制与 Python console script
  （`python -m rscmd mcp`）共用同一 stdio 循环。
- 网络层新增可配置重试策略 `RetryPolicy { retries, backoff }` 与 `download_with` /
  `download_resume_with`：默认 0 重试（保持原有行为），仅 5xx 与超时重试，退避按 2 的幂
  递增且单次封顶 30s；4xx、scheme、Range 与校验失败不重试。
- `DownloadError` 新增 `code()` 统一错误码（`NET_TIMEOUT` / `NET_HTTP_STATUS` /
  `NET_TRANSPORT` / `UNSUPPORTED_SCHEME` / `RANGE_NOT_SUPPORTED` /
  `RANGE_NOT_SATISFIABLE` / `CHECKSUM_MISMATCH` / `IO`），并登记进 `ERROR_CODE_KINDS`。
- `rscmd-runtime` 新增目录 resolver（`home::Dirs::resolve`）：按 `RSCMD_HOME` >
  Windows `LOCALAPPDATA` / macOS `~/Library/Application Support` / Linux
  `XDG_DATA_HOME` 或 `~/.local/share` 解析统一根目录，环境变量、平台与主目录均可注入；
  子目录 `cache/downloads`、`state`、`logs` 只拼接不创建，见 ADR 0001。
- `rscmd-runtime` 新增下载缓存（`cache::DownloadCache`）：URL 规范化 SHA-256 内容寻址、
  默认 TTL 7 天、256 MiB / 1024 条容量上限与写后淘汰；损坏、过期、孤立文件自动清理。
  `net::download_with_cache` 命中不重复下载，未启用或目录不可写时静默降级为直接下载。
  纯新增能力，现有工具行为与 JSON 契约不变。
- `ERROR_CODE_KINDS` 登记 `INVALID_ENCODING`（invalid_input，退出码 3），用于编解码输入非法
  或解码结果不是有效 UTF-8。
- 新增工具 `codec`（crypto 域，别名 `codetool`）：base64 / hex / URL 文本编解码，
  `--mode` 选择方式、`--decode` 反向；base64 与 hex 为手写实现（不新增依赖），
  URL 编解码复用既有 `urlencoding`。输出固定为 `{mode, direction, text, bytes}`，
  CLI 与 `--json` 共用同一结果信封。
- 新增工具 `regex`（text 域，别名 `regextool`）：匹配、分组捕获与替换，输出
  `{pattern, count, matches:[{text,start,end,groups}]}`，`--replace` 时额外输出 `text`；
  `start`/`end` 为 UTF-8 字节偏移，跨平台一致；`--size-limit` 限制自动机大小（默认 10 MiB，
  0 为不限制）。
- 新增依赖 `regex`（D1 逐个审批：线性时间无回溯引擎，用于 `regex` 工具；实测
  release 二进制由 1,613,312 字节增至 3,115,520 字节，+1,502,208 字节 ≈ +1.43 MiB）。
- `ERROR_CODE_KINDS` 登记 `INVALID_REGEX`（invalid_input，退出码 3），用于非法表达式与
  超出 `--size-limit` 的编译失败。
- 新增工具 `path`（system 域，别名 `pathtool`）：`normalize` / `join` / `relative --base` /
  `is-abs` 子命令，输出统一使用 `/` 分隔，绝对路径判定与宿主平台无关；
  `cfg(windows)` 下 `relative` 按大小写不敏感比较。
- 新增 `rscmd-tool-system` 工具域，`rscmd-tools` 增加 `system` feature（默认启用）。
- 新增工具 `which`（system 域，别名 `whichtool`）：按 PATH 顺序查找可执行文件，
  输出 `{program, path}`，未找到时 `path` 为 `null`。Windows 按 PATHEXT 追加扩展名、
  只要求普通文件；Unix 要求可执行位；含分隔符的输入按直接路径相对 cwd 解析。
  PATH、PATHEXT、平台与 cwd 均可注入，跨平台分支可在任一宿主上单测。
- 新增工具 `filesearch`（system 域，别名 `filesearchtool`）：按文件名 glob 与内容子串检索文件，
  输出 `{root, scanned, matched, truncated, results}`；结果路径相对 `ROOT` 且统一用 `/` 分隔，
  内容模式返回 `{path, line, text}`（`line` 从 1 开始，行尾 `\r` 自动去掉）。glob 由
  `fs::glob_match` 手写实现（`*`/`?` 不跨目录、`**` 跨目录且可匹配零层），未引入 `globset`；
  匹配大小写敏感，三平台一致。条数上限沿用全局 `--limit`（写入 `ToolCtx.limit`，
  未指定或 0 表示不限制），达到上限即提前停止；JSON 入口另提供同义 `limit` 字段。
  内容模式默认跳过大于 `--max-bytes`（默认 1 MiB）的文件与二进制/非 UTF-8 文件。
- 新增工具 `stat`（data 域，别名 `stattool`）：对数值列表输出
  `{count, sum, mean, median, percentiles}`；分位数用 `p/100*(n-1)` 线性插值，
  0/100 对应最小/最大值，`--percentile` 可重复或逗号分隔。空输入、非有限数值、
  超出 0–100 的分位数返回 `INVALID_ARGS`（field 分别为 `values`/`percentiles`）。
- 新增工具 `csv`（data 域，别名 `csvtool`）：CSV↔JSON 双向转换，`--to json`
  （默认）输出 `{direction, delimiter, has_header, count, truncated, rows}`，
  `--to csv` 输出 `{direction, delimiter, has_header, count, truncated, csv}`。
  表头自动判定（首行非空、唯一且非数字时视为表头，`--no-header` 可关闭）；
  分隔符 `auto` 按首行引号外出现次数在 `,` `;` `\t` `|` 中择多，也可显式指定。
  支持引号内换行/转义、CRLF 与 BOM；非 UTF-8 输入返回 `INVALID_ENCODING`。
  对象转 CSV 时列按列名字典序、`null`/缺失列写空、嵌套值写 JSON 文本。
  条数上限沿用全局 `--limit`（`ToolCtx.limit`），JSON 入口另有同义 `limit` 字段。
  依赖新增 `csv` crate（v1，默认 features，无额外启用项）。
- 新增工具 `timestamp`（data 域，别名 `timestamptool`）：Unix 秒（可为负数）与 RFC3339
  双向转换，输出 `{unix, rfc3339, offset}`；`--offset` 指定固定偏移（`+08:00`/`-05:30`/
  `Z`/`utc`），`--add` 按固定秒数加减（单位 `s`/`m`/`h`/`d`/`w`，`1d` = 86400 秒）。
  时区一律来自显式参数：不读系统时区、不做隐式本地转换，跨机器结果一致；非法输入、
  偏移与时长分别返回 `INVALID_ARGS`（field 为 `input`/`offset`/`add`）。
  依赖新增 `time` crate（v0.3，仅 `parsing` + `formatting` 两个 feature，无本地时区支持）。
- 新增工具 `urlcheck`（net 域，别名 `urlchecktool`）：探测 URL 的 HTTP 状态码、重定向链与耗时。
  `--method head|get`、`--max-redirects N`（0 表示不跟随），请求超时沿用全局 `--timeout`；
  输出 `{url, method, final_url, status, ok, redirects, elapsed_ms, content_length}`。
  4xx/5xx 视为探测完成（`ok=false`），只有传输层/超时/scheme 问题才返回
  `NET_TIMEOUT` / `NET_TRANSPORT` / `UNSUPPORTED_SCHEME`。新增 `net` 工具域 crate
  `rscmd-tool-net`（无新增第三方依赖，复用 runtime 的 reqwest）。
- `rscmd-runtime` 新增跨进程目录锁 `lock::DirLock`：用 `create_new` 独占创建
  `<dir>/<name>.lock`，`try_acquire` 立即返回 `LOCK_BUSY`，`acquire(dir, name, timeout)`
  轮询到上限返回 `LOCK_TIMEOUT`，目录只读或无法创建返回 `LOCK_UNAVAILABLE`；
  锁文件内的 pid/获取时间仅供排障，持有者 Drop 时删除（panic 亦释放，进程被强杀则残留）。
  下载缓存写同一键时用它串行化（500 ms 上限，拿不到锁放弃本次写入，下载结果不受影响）。
- `ERROR_CODE_KINDS` 登记 `LOCK_BUSY` / `LOCK_TIMEOUT` / `LOCK_UNAVAILABLE`（runtime，退出码 1）。
- 新增工具 `filedate`（system 域，别名 `filedatetool`）：读取或写入文件修改时间，输出
  `{path, mtime_unix, mtime_ns, rfc3339}`；`mtime_unix` 为整秒（可为负），`mtime_ns` 为
  秒内纳秒分量（0–999,999,999），`rfc3339` 一律按 UTC 渲染，不读取系统时区。
  `--set <UNIX秒|RFC3339>` 写入 mtime，必须显式 `--confirm`；`--dry-run` 只报告计划值、
  不改动文件；写入后重新读取 mtime 作为结果。未新增依赖（复用 workspace 已有的 `time`，
  仅 parsing/formatting）；Python 绑定无 `--confirm`，因此入口为只读。
- `ERROR_CODE_KINDS` 登记 `CONFIRM_REQUIRED`（usage，退出码 2），并新增
  `ToolCtx::require_confirm`：`--dry-run` 直接放行，否则缺少 `--confirm` 时返回该错误码。
- 新增工具 `env`（system 域，别名 `envtool`）：只读查询环境变量，不提供 setenv/删除。
  默认列出全部变量并按名字排序，`NAME` 精确过滤、`--prefix` 前缀过滤；输出
  `{count, case_insensitive, vars:[{name,value}], non_utf8}`。Windows 下变量名大小写不敏感
  （精确与前缀匹配均按 ASCII 不敏感比较），Unix 保持敏感；非 UTF-8 名称/值用 U+FFFD 替换
  并在 `non_utf8` 中列出，不报错。未引入第三方依赖。
- 新增工具 `archive`（system 域，别名 `archivetool`）：把目录打包成 zip，不加密、不删除源文件。
  `-o/--output` 必填，写文件需 `--confirm`，`--dry-run` 免确认并返回计划清单（受全局
  `--limit` 限制，超出时 `truncated=true`）；`--include`/`--exclude` 按相对路径 glob 过滤。
  输出 `{directory, output, file_count, total_bytes, dry_run}`，dry-run 额外含 `files`。
  条目按相对路径排序、路径统一 `/`、时间固定为 zip 默认值，同一输入重复打包字节一致；
  符号链接跳过；输出位于源目录内时自动排除。依赖新增 `zip` v3（`default-features = false`，
  仅 `deflate-flate2` + flate2 `rust_backend`，其余压缩与加密全部关闭）。该工具只有 argv 入口
  （`python=argv_only`，无 JSON 调用入口，但保留参数 schema 供文档）：写操作需要 `--confirm`，
  而 JSON 调用通道没有 confirm/dry-run 参数。
- 新增工具 `toml`（data 域，别名 `tomltool`）：校验 TOML 文件并转换为 JSON（默认）或 YAML，
  输出 `{path, to, text}`；`--check` 只校验语法、输出 `{path, ok: true}`（与 `--to` 互斥）。
  输入按 UTF-8 读取并去掉可选 BOM，非 UTF-8 返回 `INVALID_ENCODING`；日期时间按 TOML 原文
  渲染为字符串、不读系统时区，跨机器结果一致；`inf`/`nan` 无法用 JSON 表示、
  语法错误均返回 `PARSE_FAILED`（field 为 `path`）并带 hint。
  依赖新增 `toml` v0.9 与 `serde_norway` v0.9（YAML 渲染，后续 `yaml` 工具复用；实测 release
  二进制由 6,998,528 字节增至 7,289,856 字节，+291,328 字节 ≈ +0.28 MiB）。
- 新增工具 `yaml`（data 域，别名 `yamtool`）：校验 YAML 并转换为 JSON（默认）或 TOML，
  输出 `{path, to, text}`；`--check` 只校验语法、输出 `{path, ok: true}`（与 `--to` 互斥）。
  输入按 UTF-8 读取并去掉可选 BOM，非 UTF-8 返回 `INVALID_ENCODING`；非字符串键、重复键、
  带标签值，以及 TOML 无法表示的 `null`、非表顶层、超出 64 位有符号范围的整数与
  `.inf`/`.nan`，统一返回 `PARSE_FAILED`（field 为 `path`）并带 hint。
  复用 T14 引入的 `serde_norway`，无新增依赖；实测 release 二进制由 7,289,856 字节增至
  7,490,048 字节，+200,192 字节 ≈ +0.19 MiB。
- `ERROR_CODE_KINDS` 登记 `PARSE_FAILED`（invalid_input，退出码 3）：输入文件语法非法，
  或源格式中存在目标格式无法表示的取值（TOML `inf`/`nan`、YAML 非字符串键与重复键等）。

### 变更

- `rscmd-runtime::fs::normalize_path` 保留绝对路径根（此前会丢掉开头的 `/`，如
  `/a/../b` 得到 `a/b`），并保留 UNC 的 `//` 前缀；新增 `fs::is_absolute_path`。
  非 breaking：此前无调用方，纯字符串语义不变。

- 断点续传遇到 4xx/5xx 时按 HTTP 状态码报错（此前一律报「不支持 Range」）；
  2xx 但非 206 仍报 `RANGE_NOT_SUPPORTED`。
- **契约变更（breaking）**：`rscmd tools --json` 的 `data` 由裸数组改为 `rscmd-tools/1` 版本信封
  （`version` + `tools`），与 `rscmd schema` 的自描述风格一致。旧消费方需把 `data` 改为读取
  `data.tools`；单条元数据字段（`name`/`aliases`/`side_effects`/`python`/`gui`/`examples`）不变。
- `rscmd schema` 的每个工具条目固定输出 `aliases` 与 `side_effects`，并用契约测试锁定
  （字段此前已输出，本次只补契约与文档，属兼容扩展）。
- CLI 帮助与命令一览中的短别名改为 `doctor(d)` 形式：内建命令的名称/别名/说明集中登记在
  `registry::BUILTINS`，帮助由该表构造，`cargo xtask gen-docs` 新增 `docs/commands.md`。
- 注册表校验的保留名范围扩展到内建命令短别名（`r`/`i`/`v`/`d`）：工具名或别名与内建命令撞车时
  `cargo xtask registry-check` 直接失败，避免把重复的 clap 子命令/别名留到运行期才暴露。
- `cargo xtask registry-check` 新增确认闸门校验：工具 `META` 声明 `side_effects: SideEffects::Write`
  却未调用 `ToolCtx::require_confirm` 时直接失败（既有 `archive` / `filedate` 已合规）。
- `rscmd doctor` 的「Python 扩展」检查与 `py-fresh-check` 对齐口径：计算源码新鲜度时跳过
  `crates/xtask` 与 `crates/rscmd-cli`（二者不是扩展输入）。此前修改这两个 crate 会让 `doctor`
  误报扩展过期并以退出码 5 失败。
- Python 侧加入类型检查：选 mypy（纯 Python 实现，CI 无需额外运行时），配置在 `pyproject.toml`
  的 `[tool.mypy]`（`strict`，覆盖 `python/rscmd` 与 `tests/py`）；新增 `python/rscmd/_native.pyi`
  声明原生边界签名，`_raise_native` 标注 `NoReturn`。`cargo xtask py-type-check` 已并入 `check`。
- CI 新增 `wheel-smoke` job（ubuntu/windows/macos）：用 `cargo xtask wheel-smoke` 构建 release
  wheel、装进 `.venv` 后验证导入、成功调用与错误映射；`python` job 同时执行 mypy。两项只做测试，
  不改变发布物与发布流程。
- `cargo xtask version-check` 现在校验 `CHANGELOG.md` 存在 `[workspace.package].version` 对应段落，
  缺段直接失败并提示补写，避免发布时漏写更新日志。
- 新增 `cargo xtask code-check`：扫描 `crates/**` 中 `RscmdError::coded("...")` 字面量，未在
  `ERROR_CODE_KINDS` 登记即失败；已并入 `check-fast`，防止漏登记导致 kind 回退成 `input`（退出码 2）。
