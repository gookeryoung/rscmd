# 命令一览（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成；内建命令的短别名以括号标注，如 `rscmd doctor(d)`；
工具命令只显示规范名，别名见 [tools.md](./tools.md)。

## 内建命令

| 命令 | 说明 |
|---|---|
| `rscmd run(r)` | 运行指定工具（推荐入口） |
| `rscmd list(l)` | 列出可用工具 |
| `rscmd info(i)` | 查看单个工具的元数据 |
| `rscmd schema` | 导出全部工具的元数据 |
| `rscmd version(v)` | 输出版本信息 |
| `rscmd doctor(d)` | 输出运行环境自检信息 |
| `rscmd completion` | 生成 shell 补全脚本（bash/zsh/fish/powershell） |
| `rscmd mcp` | 以 MCP stdio server 模式运行（供模型客户端调用） |

## 工具命令

| 命令 | 说明 |
|---|---|
| `rscmd case` | 命名风格转换（snake/camel/pascal/kebab） |
| `rscmd wc` | 统计文本的行数、词数、字符数与字节数 |
| `rscmd regex` | 正则匹配、分组捕获与替换（Rust regex 语法） |
| `rscmd csv` | CSV 与 JSON 互转，自动判定表头与分隔符 |
| `rscmd json_fmt` | 校验并格式化 JSON；非法输入返回可自纠的错误 |
| `rscmd stat` | 数值统计：计数、求和、均值、中位数与分位数 |
| `rscmd timestamp` | Unix 秒与 RFC3339 互转，支持固定偏移时区与日期加减 |
| `rscmd toml` | 校验 TOML 并转换为 JSON 或 YAML |
| `rscmd yaml` | 校验 YAML 并转换为 JSON 或 TOML |
| `rscmd hash` | 计算文本或文件的 SHA-256（hex） |
| `rscmd codec` | 文本编解码：base64 / hex / URL |
| `rscmd archive` | 把目录打包成 zip（不加密，写需 --confirm，--dry-run 列清单） |
| `rscmd env` | 只读查询环境变量，支持按名字与前缀过滤 |
| `rscmd filedate` | 读取或写入文件修改时间（写需 --confirm，时间按 UTC 渲染） |
| `rscmd filesearch` | 按文件名 glob 与内容子串检索文件，支持 --limit 截断 |
| `rscmd path` | 路径规范化、拼接、相对路径与绝对路径判定 |
| `rscmd reshack` | 查看/导出/原位替换 Windows PE 资源（写需 --confirm，替换前自动备份） |
| `rscmd reshackw` | 启动 reshack 资源编辑器窗口（GUI 二进制 rscmd-reshackw，可带一个待打开文件） |
| `rscmd which` | 在 PATH 中查找可执行文件，返回绝对路径或 null |
| `rscmd urlcheck` | 探测 URL 的 HTTP 状态码、重定向链与耗时（HEAD/GET） |
