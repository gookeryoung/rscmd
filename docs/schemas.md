# 工具参数 Schema 摘要（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成；完整 schema 见 `schema/*.json`。

需要机器可读的全量自描述时，运行 `rscmd schema --json`：
`data` 为 `rscmd-schema/1` 对象，含 `version`、`tools`、`builtins`、`global` 四个字段。

## CaseArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `mode` | string | `"snake"` | - | 转换目标：snake/camel/pascal/kebab |
| `text` | string | `—` | ✅ | 待转换的字符串 |

## WcArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `text` | string | `—` | ✅ | 待统计的文本 |

## JsonFmtArgs

| 参数 | 类型 | 默认 | 必填 | 说明 |
|---|---|---|---|---|
| `json` | string | `—` | ✅ | 待处理的 JSON 文本 |
| `minify` | boolean | `false` | - | 压缩为单行 |

