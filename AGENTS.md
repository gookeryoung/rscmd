# 开发工作流

## 仓库定位

- 唯一主开发位置：Windows 下的 `F:\Dev\rscmd`。仓库不维护 WSL / Linux 本地副本，也不需要让多个工作区保持同一提交。
- 形态：Cargo workspace（`crates/`）+ maturin/PyO3 扩展（`python/`）；`schema/*.json` 与 `docs/*.md` 是生成物，由 `cargo xtask` 产出并校验。
- Linux / macOS 兼容性与三平台 wheel 打包只在 CI 中验证；本地门禁在 Windows 主副本上跑。

## 门禁分级（先定性，再选门禁）

提交前用 `git diff --name-only` + `git status --short` 判定改动类型，**按最高级别执行**；拿不准按 L2 处理（fail-safe）。

| 级别 | 判定范围（只看文件路径） | 必跑 | 跳过 |
|---|---|---|---|
| **L0 文档** | 仅 `*.md`（`docs/`、`AGENTS.md`、`CHANGELOG.md`、`README.md` 等） | `make check-docs`（`adr-check` + 生成物 `docs-check`） | 所有测试门禁 |
| **L1 配置** | 仅 `Makefile`、`Cargo.toml`、`Cargo.lock`、`clippy.toml`、`deny.toml`、`rust-toolchain.toml`、`pyproject.toml`、`uv.lock`、`.github/`、`.pre-commit-config.yaml` | `make check-fast`；**触及测试发现 / 依赖 / 打包 / CI 行为**（`Cargo.toml`、`Cargo.lock`、`pyproject.toml`、`uv.lock`、workflow）时加跑 `make test` | `cov`（除非本次显式改覆盖率口径） |
| **L2 代码** | 命中 `crates/`、`python/`、`tests/` 等 | `make flow`（`check-fast` → `test`）；阶段收尾 / 发版 / 改覆盖率口径时 `make check` + `make cov` | — |

- 只改文档或项目配置时不跑 `make flow`：`test` / `cov` 对 Markdown 与 Makefile 目标增删没有判别力，跑了只是等时间。
- 但**不能因为「主要改了文档」而降级**：同一次提交里既有代码又有文档 / 配置，按 L2 走全链。
- 分级只约束本地内环与提交前自查；CI 仍跑全量门禁，不因分级放宽。
- 禁止 `--no-verify`、禁止跳过 pre-commit（prek）；prek 对 L0 / L1 同样会触发，属预期。
- commit message 用 conventional commits（正文中文）：L0 用 `docs:`，L1 用 `chore:` / `ci:` / `build:`，L2 按改动性质（`feat:` / `fix:` / `refactor:` / `test:`）。

## 与 fcmd 的门禁语义映射

命名保持同一套肌肉记忆，实现按 Rust 工程改造：

| fcmd | rscmd | 说明 |
|---|---|---|
| `make check-docs`（gitkeep + adr） | `make check-docs`（`adr-check` + `docs-check`） | `gitkeep-check` 无对应：本仓库不用空目录占位；额外校验 `schema/`、`docs/` 生成物新鲜度 |
| `ruff check` + `ruff format --check` | `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings` | 在 `make check-fast` 内由 xtask 串起 |
| `pyrefly check` | `mypy`（`cargo xtask py-type-check`，配置在 `pyproject.toml [tool.mypy]`） | 属 `make check`，不进 `check-fast`：需要 `_native` 产物 |
| `pytest` | `cargo test --workspace` + `pytest tests/py` | Rust 侧由 `make test`；Python 侧由 `make py-check` / `make check` |
| `pytest --cov`（95%） | `cargo llvm-cov`（`--fail-under-lines 95`） | 排除 `crates/rscmd-py/` 与 `crates/xtask/`（薄绑定 / 构建脚本） |
| `.coveragerc` | `check_cov()` 的 `--ignore-filename-regex` | 口径写死在 xtask，避免两处漂移 |
| `bump-my-version` | 手动改 `[workspace.package].version` + `CHANGELOG.md` | 用 `make version-check` 校验 tag / Cargo / wheel / CHANGELOG 一致 |
| `make push` | `make push` | `cargo xtask push`：串行推送所有 remote 与 tags，任一失败非零退出 |

`cargo deny`（licenses / bans / sources 离线跑，advisories 需本地 RustSec 库）与 `cargo machete` 直接沿用 fcmd 的思路：缺失工具时打印跳过原因，不静默通过。

## 代码改动（L2）执行链

- 每次改完代码必须依次执行：`make check-fast` → `make test` → 项目文档更新 → `make push`
- 改了工具元数据 / 参数结构时加跑 `make gen-docs` 与 `make gen-schema`，否则 `check-fast` 会因生成物过期失败
- 任一步失败就修，修完重跑，全过才算完成

## 开工前（换会话 / 换 agent 必读）

1. 先读 [docs/AI_DRIVEN_DEV_FLOW.md](docs/AI_DRIVEN_DEV_FLOW.md)；工具不自动加载时也必须手动读。
2. 找当前任务的 ADR：看 [docs/adr/README.md](docs/adr/README.md) 索引（索引为空表示本项目尚无 ADR，直接按需求卡片 / 方案文档实施）。
3. 跑一次基线：代码任务 `make check-fast`，纯文档 / 配置任务 `make check-docs`；不要假设上一轮全绿。

## 收工前

1. 有 ADR 时回填「落地状态」与「交接」（做到哪期 / 下一步 / 已知坑）。
2. 更新 README / `docs/*` 中受本次改动影响的说明；生成物用 `make gen-docs` / `make gen-schema` 重生成。
3. 按「门禁分级」跑到对应级别全绿再提交（L0 / L1 不跑 `make flow`；L2 至少 `make flow`）；推送用 `make push`（打 tag 前人工确认）。

## 提交信息

- conventional commits，type 英文、描述与正文中文，单一逻辑变更一个提交；重构 / 功能 / 文档不混在同一个提交。
- 示例：`feat(工具名): 新增 xxx 能力`、`fix: 修正 yyy 的退出码`、`refactor(runtime): 抽出 zzz`、`docs: 对齐开发约定`、`chore: 升级依赖`。

## 需求移除 / 变更

1. 先清理 ADR，再改代码：删去失效的口径行 / 现状条目 / 决策行 / 测试与分期，不留「历史方案」段落，保持 ADR 与当前项目一致。
2. ADR 被新方案替代或需求作废：**直接删除**该 ADR 文件、[docs/adr/README.md](docs/adr/README.md) 索引行及代码 / 文档中的引用，不在仓库保留 `Superseded` / `Deprecated` 文档；编号不复用，追溯看 git 历史（`git log -- docs/adr/`）。
3. `make adr-check` 会拦截文件名 / 编号 / 索引 / 废弃状态不一致的 ADR。

## 文档约定

- `docs/adr/NNNN-<topic>.md`：长期决策，编号从 0001 连续；索引在 `docs/adr/README.md`。
- `docs/analysis/YYYY-MM-DD-<topic>.md`：一次性调研 / 对照 / 实施方案，不承诺长期维护。
- `docs/requirements/NNNN-*.md`：需求卡片（模板见 `docs/REQUIREMENT_CARD_TEMPLATE.md`），按需创建目录。
- `docs/tools.md`、`docs/commands.md`、`docs/global-args.md`、`docs/schemas.md`、`docs/README.md`：**生成物，勿手改**，由 `make gen-docs` 产出。
