# reshack GUI 迁移方案（reshackw）

- 日期：2026-11-17
- 状态：方案已选定路线 A（egui/eframe 独立 crate）；阶段 0 POC 已通过，待阶段 1
- 范围：仅 GUI 部分，对齐 fcmd 的 `reshackw`（Tk）。CLI `reshack` 已迁移完成，不在本方案内重复设计。
- 性质：方案 + 阶段 0 POC；阶段 1 起的正式 GUI 尚未开始。POC 代码位于 `crates/rscmd-gui`，默认不参与 CLI / wheel 构建。

## 0. 结论先行

1. **GUI 与 CLI 契约解耦**：GUI 是独立呈现层，不改变 `rscmd-result/1` / `rscmd-error/1`、退出码、schema 与 MCP 暴露范围。
2. **推荐路线为「Rust 原生 GUI（egui/eframe）+ 独立 crate」**，Windows 优先、其他平台可编译但不作为验收目标；直接复用 `reshack` 的 PE 解析/写回逻辑，不复制业务代码。
3. **不推荐直接照搬 Tk 代码**：rscmd 没有 Python GUI 层，移植 `theme.py`（45 KB）与 Tk 轮询骨架的收益低于维护成本；仅在「必须最快对齐 fcmd 交互」时才考虑 Python tkinter 兜底。
4. **分两期交付**：一期只读（打开/树/文本/十六进制/图像预览/导出），二期写入（替换/全部导出）。写操作继续沿用「确认 + 自动备份 + 原子落盘」语义。
5. 开工前必须由用户拍板 6 个决策点（见 §9），其中**技术路线、依赖审批、GUI 入口形态**是硬前置。

## 1. 已完成部分复盘

CLI `reshack` 已迁移并提交（`3f81858`），为 GUI 提供了可直接复用的业务底座：

| 能力 | 现状 | GUI 可复用性 |
|---|---|---|
| PE 头解析（PE32 / PE32+、节表、资源目录） | 手写实现，零新增依赖 | 完全复用 |
| 资源遍历（类型/名称/语言，命名资源跳过） | `ResourceWalker` | 完全复用 |
| 资源数据读取 | `PeImage::resource_data` | 需从私有提升为公开 API |
| ICO / BMP 头包装 | `bitmap_to_bmp` / `icon_to_ico` / `export_payload` | 完全复用 |
| 单资源导出 / 全部导出 | `extract` / `export_all`，写文件需 `--confirm` | 复用落盘逻辑，确认改由 GUI 对话框承担 |
| 原位替换 | 零填充 + 回写 `Size` + 备份 + 原子写 | 复用写回流程 |
| 错误码 | `PARSE_FAILED` / `IO` / `INVALID_ARGS` / `INVALID_MODE` / `RESOURCE_NOT_FOUND` / `CONFIRM_REQUIRED` | GUI 直接映射为对话框 |
| 测试与覆盖率 | 20 项单测；`reshack.rs` 行覆盖 95.71%，全仓 97.10% | 新增 GUI 需另定覆盖率口径 |

**GUI 缺口**：没有窗口运行时、没有图像解码、没有剪贴板、没有设置读写封装、没有原始字节的公开读取入口。

## 2. 已确认事实（调研结论）

1. fcmd `reshackw.py` 为 38,973 字节的 Tk 窗口，依赖 `tkinter/ttk`、`Pillow`（可选）、`threading`、`fcmd.gui.{config,theme,window}`。
2. fcmd GUI 共享骨架：`config.py`（设置 JSON、`.bak`、原子写）、`window.py`（DPI 感知、geometry 夹紧、80 ms worker 轮询、日志）、`theme.py`（45,102 字节主题体系）。
3. rscmd 当前**没有任何 GUI / 图像 / 剪贴板依赖**：workspace 依赖清单与 `Cargo.lock` 中不存在 `eframe`、`egui`、`image`、`rfd`、`arboard`、`encoding_rs`。
4. `ToolMeta.gui` 已存在，但**只有元数据消费方**：`tools --for-llm` 过滤 GUI 工具、MCP 只暴露非 GUI 且有 `run_json` 的工具；没有运行时读取该字段。
5. `reshack` 的内部类型与函数全部是私有项：`PeImage`、`load`、`resource_data`、`export_payload`、`bitmap_to_bmp`、`icon_to_ico` 等；公开面只有 `META`、`ReshackInput`、`build/run_cli/run_json/def`。
6. CLI 的 `list` 只返回元数据与头部信息，**不返回资源原始字节**；`extract` 通过写文件交付数据。GUI 预览不能只靠现有 JSON 契约。
7. 设置目录能力已具备：`rscmd_runtime::home::Dirs::resolve`（`RSCMD_HOME` > 平台默认）、`state_dir()`；原子写 `rscmd_runtime::fs::atomic_write` 已存在。
8. 图像资源只做文件头包装，不做像素解码；GUI 需要新增解码能力（`image` crate 或 Windows WIC）。
9. 剪贴板：fcmd 用 `ctypes` 写 Windows `CF_DIB`；Rust 需 `arboard`（跨平台）或 `windows` crate（仅 Windows）。
10. 门禁约束：`gen-schema` **要求每个工具都有 schema**（`ToolDef::schema` 为 `None` 会直接失败）；`arch-check` 未限制新增 crate 的依赖方向；`clippy --workspace --all-targets` 会覆盖新增成员。
11. 覆盖率口径写死在 xtask（排除 `rscmd-py/`、`xtask/`）；GUI 事件循环无法在无显示环境稳定单测，需把逻辑抽成纯函数并显式决定是否排除 GUI 壳层。

## 3. fcmd `reshackw` 功能对照

| # | fcmd 功能 | 交互细节 | rscmd 落地方式 | 复用来源 |
|---|---|---|---|---|
| 1 | 打开 PE 文件 | 文件对话框，扩展名过滤 | `rfd` 打开对话框 + `PeFile::open` | `load` |
| 2 | 最近打开 | 菜单保留 10 条，持久化 | 设置 JSON + 菜单重建 | 新增 |
| 3 | 资源树 | 按类型分组，列：名称/类型/大小/语言 | 左侧 `SidePanel` + 可折叠分组 | `resources()` |
| 4 | 文本预览 | UTF-16 BOM / 零字节比例 / UTF-8 / GBK 回退 | 纯函数 `decode_text`（编码回退见 §9） | 新增 |
| 5 | 十六进制预览 | 每行 16 字节 + ASCII，截断 4 KiB | 纯函数 `format_hex` | 新增 |
| 6 | 图像预览 | Icon/Bitmap，Pillow 解码 | `image` 解码 ICO/BMP → egui 纹理 | 复用 `export_payload` 补头 |
| 7 | 缩放 | ±、1:1、适应窗口、Ctrl+滚轮 | egui `ScrollArea` + 纹理缩放 | 新增 |
| 8 | 复制图像 | Windows 剪贴板 `CF_DIB` | `arboard` 写入图像 | 新增 |
| 9 | 另存图像 | PNG / ICO / BMP | `image` 编码 + `atomic_write` | 新增 |
| 10 | 导出选中资源 | 默认文件名、Icon/Bitmap 补头 | 复用 `export_payload` + 保存对话框 | `export_payload` |
| 11 | 替换资源 | 确认框 + 自动备份 + 原位覆盖 | 复用写回流程，确认由 egui 模态框承担 | `replace` 内部逻辑 |
| 12 | 全部导出 | 选择目录，`<stem>_resources/` | 复用 `export_all` | `export_all` |
| 13 | 属性 | 类型/名称/语言/大小/RVA/前 16 字节 | egui 模态窗口 | `resource_data` |
| 14 | 关于 | 版本与说明 | egui 模态窗口 | 新增 |
| 15 | 状态栏 + 日志 | 时间戳日志、busy 禁用按钮 | 底部面板 + 状态文本 | 新增 |
| 16 | 设置持久化 | 上次文件、最近列表、窗口 geometry | `RSCMD_HOME/state/gui/reshackw.json` | `Dirs` + `atomic_write` |

## 4. 技术选型

### 4.1 候选矩阵

| 方案 | 主要依赖 | 平台 | 业务复用 | 分发形态 | 可测性 | 主要风险 |
|---|---|---|---|---|---|---|
| **A. Rust + egui/eframe** | `eframe`、`image`、`rfd`、`arboard` | Windows 优先；macOS/Linux 可编译 | 直接调用 Rust API，最高 | 单二进制 | 纯函数可测，壳层难测 | 非原生观感；CJK 字体需自备；依赖体积需实测 |
| B. Rust + Win32（`windows`） | `windows` | 仅 Windows | 直接调用 | 单二进制 | 差（大量 unsafe） | 代码量最大；无跨平台编译 |
| C. Python tkinter 移植 | `tkinter` + `Pillow` | 三平台 | 只能走 CLI/临时文件，或新增 PyO3 原始字节接口 | 需 Python 环境 + wheel 额外依赖 | 与 fcmd 相同（壳层不测） | 违背「Rust 业务、Python 薄绑定」定位；双份 GUI 维护 |
| D. Tauri | WebView2 + 前端工具链 | 三平台 | 需 IPC 层 | 安装包体积大 | 前端可测 | 为单窗口引入 Node/WebView，明显过重 |
| E. 不迁移 | 无 | — | — | — | — | 失去 fcmd 的 GUI 对等能力 |

### 4.2 推荐：方案 A（egui/eframe）

理由：

- **业务复用最彻底**：GUI 与 CLI 共用同一套 `PeFile` 解析/写回，避免第二份 PE 实现。
- **不侵入现有分发**：GUI 放在独立 crate，默认不参与 `rscmd` 二进制、PyO3 扩展与 wheel；CLI/JSON/MCP 行为零变化。
- **单二进制、无外部运行时**：不依赖 WebView2、Node 或 Python，符合当前「静态注册表单入口」的工程形态。
- **跨平台编译**：Windows 是验收目标，macOS/Linux 至少能通过 `cargo check`，与仓库「三平台仅在 CI 验证」的策略一致。

需要接受的代价：

- 新增 4 个第三方依赖，必须走 `cargo deny`（licenses / bans / sources）与 `cargo machete` 审查。
- egui 默认字体不含 CJK，必须显式加载系统中文字体（如 `simhei.ttf` / `msyh.ttc`），否则界面显示方块；这是方案 A 的**首个 POC 验证项**。
- 观感为 immediate-mode 自绘，不是 Win32 原生控件；若用户更看重原生观感，应改选 B。

**建议先做一个时限受控的 POC（阶段 0）**：只验证「窗口能开 + 中文正常 + ICO/BMP 能解码显示 + 剪贴板可写 + `cargo deny` 通过 + release 体积可接受」。POC 不通过则回退方案 E 或 B，不进入正式分期。

## 5. 目标架构（方案 A）

### 5.1 crate 与入口

```
crates/rscmd-gui/            # 新成员，默认不构建 GUI 目标
├── Cargo.toml               # eframe/image/rfd/arboard 均为 optional；bin 需 required-features = ["gui"]
└── src/
    ├── main.rs              # 仅在有 gui feature 时启动 eframe
    ├── app.rs               # ReshackApp：状态 + 事件循环
    ├── view/                # 菜单、树、预览、对话框
    ├── preview.rs           # 纯函数：decode_text / format_hex / fit_zoom / zoom_in/out
    └── settings.rs          # 设置读写（纯函数 + IO 边界）
```

- workspace 根 `Cargo.toml` 增加 `eframe` / `image` / `rfd` / `arboard` 的 workspace 依赖，均标记 `optional = true`。
- `cargo build --workspace`、`make check-fast`、`make test` 默认**不编译 eframe**；只有 `cargo build -p rscmd-gui --features gui` 才产出 GUI。
- 入口形态：**独立二进制 `rscmd-reshackw` + `ToolDef` 拉起**（决策点 4 已定）。GUI 本体仍是独立二进制，不改 wheel 内容；`rscmd reshackw [FILE]` 只做「定位 + 启动」，不把 eframe / image / arboard 带进 CLI、PyO3 与 MCP。

### 5.2 与 CLI 的复用边界

在 `crates/rscmd-tool-system/src/reshack.rs` 中**只增加公开只读/写回 API，不改 CLI 行为**：

```rust
pub struct PeInfo { /* path, machine, subsystem, is_dll, is_64bit, image_base, entry_point */ }
pub struct ResourceInfo { /* type_id, name_id, language_id, size */ }

pub struct PeFile { /* 持有 PeImage */ }

impl PeFile {
    pub fn open(path: &Path) -> Result<Self, RscmdError>;
    pub fn info(&self) -> PeInfo;
    pub fn resources(&self) -> Vec<ResourceInfo>;
    pub fn resource_data(&self, type_id: u32, name_id: u32, language: u32) -> Result<&[u8], RscmdError>;
    pub fn export_payload(&self, type_id: u32, name_id: u32, language: u32) -> Vec<u8>;
    pub fn replace(&mut self, type_id: u32, name_id: u32, language: u32, data: &[u8]) -> Result<ReplaceOutcome, RscmdError>;
}
```

约束：

- `replace` 的**确认语义留在调用方**：CLI 的 `run_json` 继续先 `ctx.require_confirm`；GUI 先弹确认框，再调用不重复确认的写回函数。两者共用同一段「备份 + 零填充 + 回写 Size + 原子写」实现，避免行为漂移。
- 写回后 GUI 必须**重新打开文件**刷新 `resources()`，不要在旧内存镜像上继续增量改。
- GUI 不通过「调用 CLI 子进程 + 解析 JSON」来实现；预览需要原始字节，子进程方案还要处理临时文件与编码，收益为负。

### 5.3 视图与状态

- 左：资源树（按 `type_id` 分组，展开状态保留在 `ReshackApp`）。
- 右：`文本` / `十六进制` / `图像` 三个页签；图像页含缩放条（−、百分比、+、1:1、适应）。
- 底：状态栏 + 可折叠日志（最近 200 行，带时间戳）。
- 菜单：文件（打开、最近打开、退出）、资源（导出、替换、全部导出、放大/缩小/实际大小/适应、属性）、帮助（关于）。
- 文件/目录对话框统一用 `rfd`；确认/关于/属性用 egui 模态窗口。
- 耗时操作（打开、替换、全部导出、图像解码）放到 `std::thread`，通过 `std::sync::mpsc` 回传，主线程 `ctx.request_repaint()`；不引入 fcmd 的 80 ms 轮询。
- busy 期间禁用工具栏按钮，避免重复提交。

### 5.4 预览管线

1. 选中资源 → 取 `resource_data`。
2. 文本：按 `decode_text` 纯函数解码，超 512 KiB 截断并提示。
3. 十六进制：`format_hex`，每行 16 字节，超 4 KiB 截断并提示。
4. 图像：Icon/Bitmap 先补 ICO/BMP 头，再交给 `image` 解码为 RGBA；写入 `egui::TextureHandle`。
5. 缩放：`>= 100%` 用邻近采样，`< 100%` 用线性采样；适应窗口按可用区域取 `min` 比例，放大上限 8×。

### 5.5 设置持久化

- 路径：`Dirs::resolve(&DirEnv::from_process())?.state_dir()/gui/reshackw.json`；解析失败时**禁用持久化并记日志**，不阻断窗口。
- 字段：`file_path`、`recent_files`（最多 10）、`window`（宽/高/位置）、`zoom`（可选）。
- 写入：先写同目录临时文件再 rename（复用 `atomic_write`），保留一份 `.bak`；读取失败逐字段回退默认值。
- 不兼容 fcmd 的 `~/.fcmd/ui/reshackw.json`（目录语义不同）；是否做一次性导入列为决策点。

### 5.6 中文与 DPI

- DPI 由 winit/egui 自动处理，不写 fcmd 的手动 DPI 声明。
- 启动时按顺序探测系统中文字体（`simhei.ttf` → `msyh.ttc` → `simsun.ttc`），加载失败时在日志与状态栏提示「界面中文可能显示为方块」，与 fcmd 的 CJK 告警语义一致。
- 不移植 `theme.py` 的主题切换；只保留 egui 明/暗两套视觉，减少无收益的代码量。

## 6. 分阶段计划

### 阶段 0：POC 与决策（未通过则终止）

状态：**已完成并通过**，实测结果见 §13。

- 新建最小 `rscmd-gui`，打开一个 egui 窗口，加载系统中文字体。
- 解码一张内置测试 ICO 与 BMP，显示并可缩放。
- 写入剪贴板一次，验证 `arboard` 在 Windows 可用。
- 跑 `cargo deny check` 与 release 构建，记录依赖数量与体积增量。
- **验收**：窗口中文正常、图像显示正确、剪贴板可用、deny 通过；否则回退方案 E/B。

### 阶段 1：只读浏览

- 公开 `PeFile` 只读 API（`open/info/resources/resource_data/export_payload`）。
- 实现打开、最近打开、资源树、文本/十六进制预览、状态栏、日志、设置持久化。
- **验收**：`cmd.exe` / `shell32.dll` 的资源条数与 `rscmd reshack list --json` 一致；关闭重开后恢复上次文件与窗口位置。

### 阶段 2：图像与导出

- 图像页签、缩放/适应、复制图像、另存 PNG/ICO/BMP。
- 导出选中资源（默认文件名与 CLI 一致；Icon/Bitmap 自动补头）。
- **验收**：导出 ICO 与 CLI `extract` 字节一致；另存 PNG 可被系统看图工具打开。

### 阶段 3：写入

- 替换：确认框展示类型/名称/语言/原大小/新大小，确认后备份并原位写入，完成后重新加载。
- 全部导出：选择目录 → `<stem>_resources/`，完成后提示数量。
- 属性、关于、错误映射（`RESOURCE_NOT_FOUND` / `INVALID_ARGS` / `IO` / `PARSE_FAILED`）。
- **验收**：替换后 `rscmd reshack list` 的 `size` 与新数据一致；备份文件存在；取消确认不写盘；超大新数据被拒绝且原文件不变。

### 阶段 4：收尾

- 文档：README 增加 GUI 构建/运行说明；如需注册 `ToolDef`，同步 `docs/tools.md`、`docs/commands.md`、`docs/schemas.md`。
- `CHANGELOG.md` 增加条目。
- 门禁：`make flow`；阶段收尾 `make check` + `make cov`；生成物重生成。
- CI：可选增加 Windows GUI 构建 job（只构建，不运行窗口）。

## 7. 代码与文档改动清单

| 文件 | 改动 | 阶段 |
|---|---|---|
| `crates/rscmd-tool-system/src/reshack.rs` | 抽出 `PeFile` 公开 API；CLI 改为调用同一内部实现 | 1 |
| `crates/rscmd-gui/Cargo.toml` | 新 crate；GUI 依赖 optional；`[[bin]] required-features = ["gui"]` | 0 |
| `crates/rscmd-gui/src/*` | `app` / `view` / `preview` / `settings` | 0–3 |
| 根 `Cargo.toml` | workspace 依赖与成员 | 0 |
| `crates/xtask/src/main.rs` | 仅当注册 `ToolDef` 时：处理无参数 GUI 工具 schema；确认覆盖率口径 | 3（条件） |
| `crates/rscmd-tools/src/lib.rs` | 仅当注册 `ToolDef` 时增加 feature 与断言 | 3（条件） |
| `docs/tools.md`、`docs/commands.md`、`docs/schemas.md`、`schema/*.json` | 仅当注册 `ToolDef` 时重新生成 | 3（条件） |
| `README.md`、`CHANGELOG.md` | GUI 构建/运行说明与变更条目 | 4 |

## 8. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| egui 无 CJK 字体 | 中文界面不可用 | 阶段 0 POC 必验；探测系统字体，失败给出明确提示 |
| 新增依赖体积/编译时间 | 本地与 CI 变慢 | GUI 依赖 optional；默认门禁不编译；release 构建单独 job |
| `cargo deny` 许可或来源不合规 | 门禁失败 | 阶段 0 先跑 deny；必要时更换 crate |
| 图像解码差异 | ICO 多尺寸、DIB 缺文件头导致显示异常 | 复用 `export_payload` 补头；为 `bitmap_to_bmp` / `icon_to_ico` 补真实样本测试 |
| 文本编码（GBK） | 中文资源显示乱码 | 引入 `encoding_rs` 或用「UTF-8 + UTF-16LE + 替换字符」降级；列为决策点 |
| 事件循环不可单测 | 覆盖率下降 | 逻辑抽成 `preview.rs`/`settings.rs` 纯函数；壳层是否排除由用户拍板，禁止静默加 pragma |
| 替换期间文件被外部修改 | 写回错位或覆盖他人改动 | 写前重新读取并校验资源条数与目标 `entry_offset`；不一致则拒绝并提示 |
| 大 PE 文件全量读入 | 内存占用升高 | 沿用 CLI 现状（整文件读入）；在 GUI 中限制预览解码尺寸，避免重复分配纹理 |
| 设置目录与 fcmd 不同 | 用户旧设置不迁移 | 可提供一次性导入选项；默认不读取 `~/.fcmd` |

## 9. 需用户拍板的决策点

1. **是否迁移 GUI**：方案 E（不迁移，保留 CLI）仍是合法选项；若迁移，是否接受方案 A（egui）作为主路线。
2. **技术路线**：A egui / B Win32 原生 / C Python tkinter 兜底 / E 不迁移。推荐 A，理由是业务复用与单二进制；若更看重原生观感选 B，若只看重最快对齐 fcmd 选 C。
3. **依赖审批**：`eframe`、`image`、`rfd`、`arboard`（以及可能的中文编码 crate）。需明确允许新增，或限定「仅 Windows 目标」。
4. **GUI 入口形态：已定（2026-11-17）**：保留独立二进制 `rscmd-reshackw` 承载 GUI，另注册 `ToolDef { name: "reshackw", gui: true, python: Hidden }`（落点 `crates/rscmd-tool-system/src/reshackw.rs`）用 `rscmd reshackw [FILE]` 定位并拉起。定位顺序 `RSCMD_RESHACKW` → 与 `rscmd` 同目录的 `rscmd-reshackw[.exe]`；找不到返回 `GUI_UNAVAILABLE`（退出码 1）并提示 `cargo build -p rscmd-gui --features gui`。`run_json` 为 `None`，MCP 与 `tools --for-llm` 不暴露；schema 保留以满足 `gen-schema` 的全工具覆盖要求。
5. **覆盖率口径**：GUI 壳层（`app.rs`/`view/`）是否排除出 llvm-cov；若排除，必须在 xtask 中写明文件与理由，且 `preview.rs`/`settings.rs` 仍须单测。
6. **设置兼容**：是否一次性导入 fcmd 的 `~/.fcmd/ui/reshackw.json`，还是完全使用 `RSCMD_HOME/state/gui/reshackw.json`。

## 10. 不照搬清单

1. **`theme.py` 主题体系**：45 KB 的色板、主题切换与 Tk 专属 hack 不迁移；egui 只用明/暗两套。
2. **80 ms 轮询队列**：egui 有事件驱动与 channel，无需复刻 `root.after` 轮询。
3. **tkinter / Pillow 依赖**：不把 Python GUI 栈引入 rscmd 的必装依赖。
4. **多窗口/Toplevel 体系**：只保留主窗口 + 模态对话框。
5. **Tk geometry 字符串解析**：egui 直接保存窗口尺寸与位置，不复刻 `WxH+X+Y` 解析逻辑。
6. **在 GUI 内复制 PE 解析**：所有解析/写回只走 `rscmd-tool-system` 的单一实现。

## 11. 验收与门禁

自动化：

- `cargo test -p rscmd-gui`：`preview` 的文本解码、十六进制格式、缩放计算、适应窗口；`settings` 的读写与损坏回退。
- `cargo test -p rscmd-tool-system`：公开 `PeFile` API 的只读与写回回归，不削弱现有 20 项测试。
- `make check-fast`：fmt + clippy + registry/code/arch/adr/docs/schema；GUI 默认不编译 eframe。
- `make flow`（每阶段）；阶段收尾 `make check` + `make cov`。
- 若注册 `ToolDef`：`make gen-docs` + `make gen-schema`，并确认 `docs-check` / `schema-check` 通过。

Windows 手动验收清单：

1. 打开 `C:\Windows\System32\cmd.exe` 与 `shell32.dll`，资源条数与 CLI `list` 一致。
2. 选中 Icon 与 Bitmap，三个页签均正常；缩放、1:1、适应窗口、Ctrl+滚轮可用。
3. 复制图像后可在画图/Word 中粘贴；另存 PNG/ICO/BMP 可被系统打开。
4. 导出单个资源与 CLI `extract` 字节一致。
5. 替换资源：取消确认不写盘；确认后备份存在、文件大小不变、CLI `list` 的 `size` 更新。
6. 全部导出：目录结构与 `<stem>_resources/` 命名正确。
7. 关闭重开：恢复上次文件、最近列表与窗口位置。
8. 错误路径：非 PE 文件、只读文件、超大新数据均有明确提示，不崩溃。

## 12. 关键证据与复现命令

```bash
# POC 前的依赖基线（默认构建仍不编译 GUI 依赖）
cargo tree -p rscmd-cli
cargo tree -p rscmd-tool-system
rg -n 'name = "(eframe|egui|image|rfd|arboard)"' Cargo.lock

# POC 自检与窗口冒烟
cargo run -q -p rscmd-gui --features gui -- --self-test --open %TEMP%\rscmd_poc_icon.ico
cargo build --release -p rscmd-gui --features gui
cargo deny check licenses bans sources
cargo machete

# CLI 侧已有能力（GUI 的对齐基准）
rscmd reshack list C:\Windows\System32\cmd.exe
rscmd reshack extract C:\Windows\System32\cmd.exe 3 1 -o icon.ico --confirm
rscmd reshack replace .\app.exe 3 1 .\icon.ico --dry-run

# fcmd 侧事实来源
rg -n "class ReshackWindow|def _build_widgets|def _build_menu|def _show_preview|def _replace_selected|def _export_all" F:\Dev\fcmd\src\fcmd\apps\reshack\reshackw.py
rg -n "def (create_root|start_worker|append_log)|WORKER_INTERVAL_MS" F:\Dev\fcmd\src\fcmd\gui\window.py
```

## 13. 阶段 0 POC 实测结果（2026-11-17）

### 13.1 代码与隔离

- 新增 `crates/rscmd-gui`：`src/lib.rs`（纯逻辑，7 项单测）与 `src/main.rs`（egui 窗口 + `--self-test`）。
- `gui = ["dep:eframe", "dep:image", "dep:arboard"]`；bin `rscmd-reshackw` 使用 `required-features = ["gui"]`。
- 默认 `cargo build / test / clippy --workspace` 不编译 eframe；CLI、PyO3 扩展与 wheel 未受影响。
- 根 `Cargo.toml` 新增 3 个 workspace 依赖，均为 `optional`。

### 13.2 依赖与 MSRV

| 依赖 | 实际版本 | MSRV | 启用的 feature |
|---|---|---|---|
| `eframe` | 0.29.1 | 1.76 | `default_fonts` / `glow` / `wayland` / `x11`（未启用 accesskit / wgpu） |
| `image` | 0.25.10 | 1.70 | `bmp` / `ico` / `png` |
| `arboard` | 3.7.0 | 1.71 | `image-data` |

- 在 workspace `rust-version = 1.78` 下，Cargo 选择的是 eframe 0.29.1，而不是更新的 0.37；后续升级需先评估 MSRV。
- `Cargo.lock` 新增 198 个包条目（eframe/glutin/winit 与 image 的可选编解码器占多数），无既有包版本变化；默认构建不会编译未启用 feature 的依赖。

### 13.3 自检输出

```text
font: ok (C:\WINDOWS\Fonts\simhei.ttf, 9745792 bytes)
image bmp: ok (32×32)
image ico: ok (16×16)
image C:\Users\zhou\AppData\Local\Temp\rscmd_poc_icon.ico: ok (48×48)
text utf16le: ok
clipboard: ok
POC self-test: OK
```

真实资源路径：`cmd.exe` 的 `Icon 3/8/1033` 先由 CLI 导出为 ICO，再由 POC 解码为 48×48，证明「CLI 导出 → GUI 预览」链路可用。

### 13.4 窗口冒烟

- release 版 `rscmd-reshackw.exe` 启动后进程存活 3 秒（随后手动关闭），窗口创建与事件循环正常。
- POC 未做像素级视觉断言；中文渲染与布局仍需人工看一眼，正式阶段纳入手动验收清单。

### 13.5 体积与门禁

| 项 | 结果 |
|---|---|
| `rscmd-reshackw.exe`（release） | 4,798,976 字节 |
| `rscmd.exe`（release，全工具） | 7,662,080 字节 |
| `make flow` | 通过；新增 7 项单测 |
| `make check` | 通过；540 项 Rust 测试（1 skipped）、39 项 pytest、mypy 无问题 |
| `make cov` | 通过；总行覆盖 97.02%，`rscmd-gui/src/lib.rs` 行覆盖 91.18% |
| `cargo deny check licenses bans sources` | 通过（仅既有重复版本警告） |
| `cargo machete` | 未发现未使用依赖 |

说明：GUI 二进制是独立程序，不含全部 CLI 工具，因此不能把两者差值直接当作「GUI 成本」；但它证明单二进制体积在可接受范围内。

覆盖率口径注意：`make cov` 未启用 `gui` feature，`src/main.rs` 不进入统计；若后续 CI 用 `--features gui` 跑覆盖率，需要按决策点 5 明确排除 `crates/rscmd-gui/src/main.rs`，或只做构建冒烟。

### 13.6 未覆盖项与阶段 1 前置

- 未实现文件对话框（`rfd`）、PE 资源树、文本/十六进制页签、导出/替换；POC 只验证技术可行性。
- GBK 解码未引入；TTC 字体仅按候选顺序回退，实测使用 `simhei.ttf`（9,745,792 字节）。
- GUI 入口已能由 `rscmd reshackw [FILE]` 拉起（拉起的是同目录 `rscmd-reshackw`），MCP 与 `tools --for-llm` 仍不暴露该 GUI 工具；但 GUI 本体仍是 POC，只能预览图片、不能直接打开 PE 资源树。
- 阶段 1 第一件事：把 `reshack.rs` 的 `PeImage` 提升为公开 `PeFile` API，并接入 `rfd` 打开对话框。
