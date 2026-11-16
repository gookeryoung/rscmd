# 命令一览（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成；短别名以括号标注，如 `rscmd doctor(d)`。

## 内建命令

| 命令 | 说明 |
|---|---|
| `rscmd run(r)` | 运行指定工具（推荐入口） |
| `rscmd tools` | 列出可用工具 |
| `rscmd info(i)` | 查看单个工具的元数据 |
| `rscmd schema` | 导出全部工具的元数据 |
| `rscmd version(v)` | 输出版本信息 |
| `rscmd doctor(d)` | 输出运行环境自检信息 |
| `rscmd completion` | 生成 shell 补全脚本（bash/zsh/fish/powershell） |
| `rscmd mcp` | 以 MCP stdio server 模式运行（供模型客户端调用） |

## 工具命令

| 命令 | 说明 |
|---|---|
| `rscmd case(casetool)` | 命名风格转换（snake/camel/pascal/kebab） |
| `rscmd wc` | 统计文本的行数、词数、字符数与字节数 |
| `rscmd regex(regextool)` | 正则匹配、分组捕获与替换（Rust regex 语法） |
| `rscmd json_fmt(jsonfmt)` | 校验并格式化 JSON；非法输入返回可自纠的错误 |
| `rscmd hash(hashtool)` | 计算文本或文件的 SHA-256（hex） |
| `rscmd codec(codetool)` | 文本编解码：base64 / hex / URL |
| `rscmd path(pathtool)` | 路径规范化、拼接、相对路径与绝对路径判定 |
| `rscmd which(whichtool)` | 在 PATH 中查找可执行文件，返回绝对路径或 null |
