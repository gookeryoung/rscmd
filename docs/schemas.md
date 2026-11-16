# 工具参数 Schema 摘要（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成；完整 schema 见 `schema/*.json`。

需要机器可读的全量自描述时，运行 `rscmd schema --json`：
`data` 为 `rscmd-schema/1` 对象，含 `version`、`tools`、`builtins`、`global` 四个字段。

只列工具时运行 `rscmd list --json`：`data` 为 `rscmd-tools/1` 对象，含 `version` 与 `tools`，
每个条目与 `schema.tools` 同源（含 `aliases` 与 `side_effects`）。

## CaseArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `mode` | string | `"snake"` | - | 转换目标：snake/camel/pascal/kebab |
| `text` | string | `—` | ✅ | 待转换的字符串 |

## WcArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `text` | string | `—` | ✅ | 待统计的文本 |

## RegexArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `file` | — | `null` | - | 从文件读取待匹配文本（与 TEXT 二选一） |
| `pattern` | string | `—` | ✅ | 正则表达式（Rust regex 语法） |
| `replace` | — | `null` | - | 替换模板（支持 $1 / ${name}）；提供时额外输出替换后的文本 |
| `size_limit` | integer | `10485760` | - | 自动机大小上限（字节），0 表示不限制 |
| `text` | — | `null` | - | 待匹配文本（与 --file 二选一） |

## CsvArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `delimiter` | string | `"auto"` | - | 分隔符：`auto`（默认）、`comma`、`semicolon`、`tab`、`pipe` 或单个 ASCII 字符 |
| `input` | — | `null` | - | 输入文件路径（与 --text 二选一） |
| `limit` | — | `null` | - | 最多输出的数据行数，0 或不提供表示不限制；CLI 走全局 `--limit`（ToolCtx.limit） |
| `no_header` | boolean | `false` | - | CSV→JSON：首行不是表头，按数组输出 |
| `text` | — | `null` | - | 直接传入文本（与 INPUT 二选一） |
| `to` | string | `"json"` | - | 转换方向：`json` = CSV→JSON，`csv` = JSON→CSV |

## JsonFmtArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `json` | string | `—` | ✅ | 待处理的 JSON 文本 |
| `minify` | boolean | `false` | - | 压缩为单行 |

## StatArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `percentiles` | array | `[]` | - | 需要计算的分位数（0–100，可重复或逗号分隔） |
| `values` | array | `—` | ✅ | 待统计的数值，至少一个；支持负数 |

## TimestampArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `add` | — | `null` | - | 加减时长：`1d`、`-2h`、`30m`、`1w`（单位 s/m/h/d/w，按固定秒数换算） |
| `input` | string | `—` | ✅ | Unix 秒（可为负数）或 RFC3339 时间 |
| `offset` | string | `"+00:00"` | - | 输出时区偏移：`+08:00`、`-05:30`、`Z` 或 `utc`；不读系统时区 |

## TomlArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `check` | boolean | `false` | - | 只校验语法，不输出转换结果 |
| `file` | string | `—` | ✅ | 待读取的 TOML 文件（UTF-8，可带 BOM） |
| `to` | — | `null` | - | 输出格式：json（默认）或 yaml |

## YamlArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `check` | boolean | `false` | - | 只校验语法，不输出转换结果 |
| `file` | string | `—` | ✅ | 待读取的 YAML 文件（UTF-8，可带 BOM） |
| `to` | — | `null` | - | 输出格式：json（默认）或 toml |

## HashArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `file` | — | `null` | - | 读取文件内容计算摘要（与 TEXT 二选一） |
| `text` | — | `null` | - | 待计算摘要的文本（与 --file 二选一） |

## CodecArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `decode` | boolean | `false` | - | 解码（默认编码） |
| `mode` | string | `"base64"` | - | 编解码方式：base64/hex/url |
| `text` | string | `—` | ✅ | 待编码或解码的文本 |

## ArchiveArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `directory` | string | `—` | ✅ | 要打包的目录 |
| `exclude` | array | `—` | ✅ | 排除匹配的 glob（相对 DIR，可重复） |
| `include` | array | `—` | ✅ | 只打包匹配的 glob（相对 DIR，可重复；`*`/`?` 不跨目录，`**` 跨目录） |
| `output` | string | `—` | ✅ | 输出 zip 路径（必填，不猜默认值） |

## EnvArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `name` | — | `null` | - | 变量名（Windows 下不区分大小写）；不给出则列出全部 |
| `prefix` | — | `null` | - | 变量名前缀过滤（Windows 下不区分大小写） |

## FiledateArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `path` | string | `—` | ✅ | 目标文件路径 |
| `set` | — | `null` | - | 写入修改时间：Unix 秒（可为负数）或 RFC3339；不给出则只读取 |

## FilesearchArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `content` | — | `null` | - | 内容子串（逐行匹配、大小写敏感） |
| `limit` | — | `null` | - | 结果条数上限，0 或不提供表示不限制；CLI 走全局 `--limit`（ToolCtx.limit），
 此字段只服务 JSON 入口 |
| `max_bytes` | integer | `1048576` | - | 内容检索时单文件字节上限，超过则跳过 |
| `name` | — | `null` | - | 文件名 glob：`*`/`?` 不跨目录，`**` 跨目录；含分隔符时匹配相对路径，否则只匹配文件名 |
| `root` | string | `—` | ✅ | 检索根目录 |

## PathInput

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `base` | — | `null` | - | relative 的基准路径 |
| `op` | string | `—` | ✅ | 操作：normalize/join/relative/is-abs |
| `parts` | array | `[]` | - | join 的路径片段 |
| `path` | — | `null` | - | 待处理的路径（normalize/relative/is-abs） |

## ReshackInput

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `file` | string | `—` | ✅ | PE 文件路径（exe/dll/sys 等） |
| `input` | — | `null` | - | 新资源数据文件（replace 必填） |
| `language` | integer | `0` | - | 语言 ID，默认 0 |
| `name_id` | — | `null` | - | 资源名称 ID（extract / replace 必填） |
| `op` | string | `—` | ✅ | 操作：list / extract / replace / export-all |
| `output` | — | `null` | - | 输出文件路径（extract）；缺省为当前目录下的默认文件名 |
| `output_dir` | — | `null` | - | 输出目录（export-all）；缺省为 PE 文件同目录下的 `<文件名>_resources` |
| `type_id` | — | `null` | - | 资源类型 ID（extract / replace 必填，如 3=Icon、16=Version、24=Manifest） |

## ReshackwArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `file` | — | `—` | - | 启动时打开的文件（PE 或已导出的 ICO/BMP/PNG）；缺省只开窗口 |

## WhichArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `program` | string | `—` | ✅ | 要查找的程序名（可含相对或绝对路径） |

## UrlcheckArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `max_redirects` | integer | `5` | - | 最多跟随的重定向次数；0 表示不跟随 |
| `method` | string | `"head"` | - | 请求方法：head 或 get |
| `url` | string | `—` | ✅ | 待探测的 URL（仅支持 http:// 与 https://） |

