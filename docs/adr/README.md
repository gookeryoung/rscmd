# 架构决策记录（ADR）

长期有效的架构决策在此留痕；编号不复用，被替代或作废时**直接删除**文件与索引行（追溯看 git 历史）。

`make adr-check` 校验：文件名 `NNNN-<topic>.md`、编号从 0001 连续、每条都在本索引登记、
首行形如 `# ADR NNNN：<标题>`、不保留 Superseded / Deprecated 状态的 ADR。

| 编号 | 主题 | 状态 |
|---|---|---|
| [0001](0001-rscmd-home.md) | 配置、状态与缓存目录（`RSCMD_HOME`） | 已批准（已落地） |
