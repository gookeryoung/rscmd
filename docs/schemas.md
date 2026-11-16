# 工具参数 Schema 摘要（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成；完整 schema 见 `schema/*.json`。

需要机器可读的全量自描述时，运行 `rscmd schema --json`：
`data` 为 `rscmd-schema/1` 对象，含 `version`、`tools`、`builtins`、`global` 四个字段。

只列工具时运行 `rscmd tools --json`：`data` 为 `rscmd-tools/1` 对象，含 `version` 与 `tools`，
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

## JsonFmtArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `json` | string | `—` | ✅ | 待处理的 JSON 文本 |
| `minify` | boolean | `false` | - | 压缩为单行 |

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

## WhichArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `program` | string | `—` | ✅ | 要查找的程序名（可含相对或绝对路径） |

