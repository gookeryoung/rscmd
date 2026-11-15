# 工具参数 Schema 摘要（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成；完整 schema 见 `schema/*.json`。

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

