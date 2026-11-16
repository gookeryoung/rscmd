# 工具索引（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成；修改工具元数据后需重新生成。

| 工具 | 别名 | 摘要 | 副作用 | Python |
|---|---|---|---|---|
| `case` | `casetool` | 命名风格转换（snake/camel/pascal/kebab） | `none` | `structured` |
| `wc` | - | 统计文本的行数、词数、字符数与字节数 | `none` | `structured` |
| `regex` | `regextool` | 正则匹配、分组捕获与替换（Rust regex 语法） | `read` | `structured` |
| `json_fmt` | `jsonfmt` | 校验并格式化 JSON；非法输入返回可自纠的错误 | `none` | `structured` |
| `hash` | `hashtool` | 计算文本或文件的 SHA-256（hex） | `read` | `structured` |
| `codec` | `codetool` | 文本编解码：base64 / hex / URL | `none` | `structured` |
| `filesearch` | `filesearchtool` | 按文件名 glob 与内容子串检索文件，支持 --limit 截断 | `read` | `structured` |
| `path` | `pathtool` | 路径规范化、拼接、相对路径与绝对路径判定 | `none` | `structured` |
| `which` | `whichtool` | 在 PATH 中查找可执行文件，返回绝对路径或 null | `read` | `structured` |
