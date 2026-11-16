//! `reshack`：查看、导出与原位替换 Windows PE 资源。
//!
//! 子命令与 fcmd 的 `reshack` 对齐：`list` / `extract` / `replace` / `export-all`。
//! PE 解析与写回全部在本模块内完成（不依赖 pefile），可在任意平台处理 PE 文件；
//! 输入只接受扩展名受支持且带 `MZ` / `PE\0\0` 签名的文件。
//!
//! 写回限制与 fcmd 一致：只能原位覆盖，不能扩容。新数据大于原资源时按
//! `INVALID_ARGS` 拒绝；小于等于时零填充到原大小并回写资源数据项的 `Size`，
//! 保证后续资源偏移不变。`extract` / `replace` / `export-all` 都会写文件，
//! 因此需要 `--confirm`；`--dry-run` 只返回计划，不落盘。`replace` 写入前先复制
//! `<文件名>.backup`（已存在则递增编号），落盘采用「临时文件 + rename」，
//! 失败时原文件保持完整。
//!
//! 导出时 Icon（RT_ICON）与 Bitmap（RT_BITMAP）补 ICO / BMP 文件头，其余资源原样导出；
//! 命名资源（字符串 ID）与 fcmd 一样跳过，只处理数字 ID。

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use clap::{ArgMatches, Command};
use rscmd_core::{PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta};
use rscmd_runtime::fs::atomic_write;
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "reshack",
    aliases: &["reshacktool"],
    summary: "查看/导出/原位替换 Windows PE 资源（写需 --confirm，替换前自动备份）",
    side_effects: SideEffects::Write,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd reshack list app.exe",
        "rscmd reshack extract app.exe 3 1 -o icon.ico --confirm",
        "rscmd reshack replace app.exe 3 1 icon.ico --dry-run",
        "rscmd reshack export-all app.exe -d app_resources --confirm",
    ],
};

/// 支持的 PE 扩展名（与 fcmd 的 `PE_EXTENSIONS` 一致）。
const PE_EXTENSIONS: &[&str] = &["exe", "dll", "sys", "ocx", "scr", "cpl", "ax", "acm", "drv"];

const RT_BITMAP: u32 = 2;
const RT_ICON: u32 = 3;

/// 单个资源目录最多接受的条目数，避免损坏文件导致解析爆炸。
const MAX_DIRECTORY_ENTRIES: usize = 4096;
/// 单个文件最多接受的资源条数。
const MAX_RESOURCES: usize = 65_536;
/// PE 规范允许的最大节数量。
const MAX_SECTIONS: usize = 96;

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ReshackInput {
    /// 操作：list / extract / replace / export-all
    pub op: String,
    /// PE 文件路径（exe/dll/sys 等）
    pub file: String,
    /// 资源类型 ID（extract / replace 必填，如 3=Icon、16=Version、24=Manifest）
    #[serde(default)]
    pub type_id: Option<u32>,
    /// 资源名称 ID（extract / replace 必填）
    #[serde(default)]
    pub name_id: Option<u32>,
    /// 语言 ID，默认 0
    #[serde(default)]
    pub language: u32,
    /// 输出文件路径（extract）；缺省为当前目录下的默认文件名
    #[serde(default)]
    pub output: Option<String>,
    /// 新资源数据文件（replace 必填）
    #[serde(default)]
    pub input: Option<String>,
    /// 输出目录（export-all）；缺省为 PE 文件同目录下的 `<文件名>_resources`
    #[serde(default)]
    pub output_dir: Option<String>,
}

#[derive(Debug, clap::Args)]
struct ListArgs {
    /// PE 文件路径
    #[arg(value_name = "FILE")]
    file: String,
}

#[derive(Debug, clap::Args)]
struct ExtractArgs {
    /// PE 文件路径
    #[arg(value_name = "FILE")]
    file: String,
    /// 资源类型 ID（3=Icon、16=Version、24=Manifest 等）
    #[arg(value_name = "TYPE")]
    type_id: u32,
    /// 资源名称 ID
    #[arg(value_name = "NAME")]
    name_id: u32,
    /// 语言 ID
    #[arg(long, value_name = "LANG", default_value_t = 0)]
    language: u32,
    /// 输出文件路径；缺省为当前目录下的默认文件名
    #[arg(short = 'o', long, value_name = "PATH")]
    output: Option<String>,
}

#[derive(Debug, clap::Args)]
struct ReplaceArgs {
    /// PE 文件路径
    #[arg(value_name = "FILE")]
    file: String,
    /// 资源类型 ID
    #[arg(value_name = "TYPE")]
    type_id: u32,
    /// 资源名称 ID
    #[arg(value_name = "NAME")]
    name_id: u32,
    /// 新资源数据文件
    #[arg(value_name = "DATA")]
    input: String,
    /// 语言 ID
    #[arg(long, value_name = "LANG", default_value_t = 0)]
    language: u32,
}

#[derive(Debug, clap::Args)]
struct ExportAllArgs {
    /// PE 文件路径
    #[arg(value_name = "FILE")]
    file: String,
    /// 输出目录；缺省为 PE 文件同目录下的 `<文件名>_resources`
    #[arg(short = 'd', long, value_name = "DIR")]
    output_dir: Option<String>,
}

pub fn build() -> Command {
    Command::new(META.name)
        .about(META.summary)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(<ListArgs as clap::Args>::augment_args(
            Command::new("list").about("列出 PE 头信息与全部资源"),
        ))
        .subcommand(<ExtractArgs as clap::Args>::augment_args(
            Command::new("extract").about("导出单个资源（写文件需 --confirm）"),
        ))
        .subcommand(<ReplaceArgs as clap::Args>::augment_args(
            Command::new("replace").about("用新数据原位替换资源（需 --confirm，自动备份）"),
        ))
        .subcommand(<ExportAllArgs as clap::Args>::augment_args(
            Command::new("export-all").about("导出全部资源到目录（需 --confirm）"),
        ))
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    match matches.subcommand() {
        Some(("list", sub)) => {
            let args = <ListArgs as clap::FromArgMatches>::from_arg_matches(sub)
                .map_err(RscmdError::from_clap)?;
            run(
                ctx,
                ReshackInput {
                    op: "list".to_string(),
                    file: args.file,
                    ..ReshackInput::default()
                },
            )
        }
        Some(("extract", sub)) => {
            let args = <ExtractArgs as clap::FromArgMatches>::from_arg_matches(sub)
                .map_err(RscmdError::from_clap)?;
            run(
                ctx,
                ReshackInput {
                    op: "extract".to_string(),
                    file: args.file,
                    type_id: Some(args.type_id),
                    name_id: Some(args.name_id),
                    language: args.language,
                    output: args.output,
                    ..ReshackInput::default()
                },
            )
        }
        Some(("replace", sub)) => {
            let args = <ReplaceArgs as clap::FromArgMatches>::from_arg_matches(sub)
                .map_err(RscmdError::from_clap)?;
            run(
                ctx,
                ReshackInput {
                    op: "replace".to_string(),
                    file: args.file,
                    type_id: Some(args.type_id),
                    name_id: Some(args.name_id),
                    language: args.language,
                    input: Some(args.input),
                    ..ReshackInput::default()
                },
            )
        }
        Some(("export-all", sub)) => {
            let args = <ExportAllArgs as clap::FromArgMatches>::from_arg_matches(sub)
                .map_err(RscmdError::from_clap)?;
            run(
                ctx,
                ReshackInput {
                    op: "export-all".to_string(),
                    file: args.file,
                    output_dir: args.output_dir,
                    ..ReshackInput::default()
                },
            )
        }
        _ => Err(RscmdError::coded("INVALID_MODE", "缺少 reshack 子命令")
            .with_field("op")
            .with_expected("list / extract / replace / export-all")),
    }
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: ReshackInput = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(ctx: &mut ToolCtx, input: ReshackInput) -> Result<Value, RscmdError> {
    match input.op.as_str() {
        "list" => {
            let image = load(Path::new(&input.file))?;
            Ok(list(&image, &input.file))
        }
        "extract" => extract(ctx, &input),
        "replace" => replace(ctx, &input),
        "export-all" => export_all(ctx, &input),
        other => Err(
            RscmdError::coded("INVALID_MODE", format!("未知的 op: {other}"))
                .with_field("op")
                .with_expected("list / extract / replace / export-all"),
        ),
    }
}

#[derive(Debug, Clone)]
struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_size: u32,
    raw_pointer: u32,
}

#[derive(Debug, Clone)]
struct ResourceEntry {
    type_id: u32,
    name_id: u32,
    language_id: u32,
    data_rva: u32,
    size: u32,
    /// `IMAGE_RESOURCE_DATA_ENTRY` 的文件偏移，替换时用于回写 `Size`。
    entry_offset: usize,
    /// 资源数据的文件偏移；RVA 无法映射或越界时为 `None`。
    data_offset: Option<usize>,
}

#[derive(Debug)]
struct PeImage {
    path: PathBuf,
    data: Vec<u8>,
    machine: u16,
    subsystem: u16,
    is_dll: bool,
    is_64bit: bool,
    image_base: u64,
    entry_point: u32,
    resources: Vec<ResourceEntry>,
}

impl PeImage {
    fn parse(data: Vec<u8>, path: &Path) -> Result<Self, RscmdError> {
        let truncated = || parse_error(path, "PE 头被截断或不完整");
        let pe_offset = read_u32(&data, 0x3C)
            .ok_or_else(|| parse_error(path, "DOS 头被截断，读不到 PE 偏移"))?
            as usize;
        if read_u32(&data, pe_offset) != Some(0x0000_4550) {
            return Err(parse_error(path, "PE 签名缺失或偏移越界"));
        }

        let coff = pe_offset + 4;
        let machine = read_u16(&data, coff).ok_or_else(truncated)?;
        let section_count = read_u16(&data, coff + 2).ok_or_else(truncated)? as usize;
        let optional_size = read_u16(&data, coff + 16).ok_or_else(truncated)? as usize;
        let characteristics = read_u16(&data, coff + 18).ok_or_else(truncated)?;
        if section_count > MAX_SECTIONS {
            return Err(parse_error(path, "节数量异常"));
        }

        let optional = coff + 20;
        let magic = read_u16(&data, optional).ok_or_else(truncated)?;
        let (is_64bit, image_base, directory_count_offset, directory_offset) = match magic {
            0x10B => (
                false,
                u64::from(read_u32(&data, optional + 28).ok_or_else(truncated)?),
                optional + 92,
                optional + 96,
            ),
            0x20B => (
                true,
                read_u64(&data, optional + 24).ok_or_else(truncated)?,
                optional + 108,
                optional + 112,
            ),
            _ => return Err(parse_error(path, "可选头 Magic 既不是 PE32 也不是 PE32+")),
        };
        let entry_point = read_u32(&data, optional + 16).ok_or_else(truncated)?;
        let size_of_headers = read_u32(&data, optional + 60).ok_or_else(truncated)?;
        let subsystem = read_u16(&data, optional + 68).ok_or_else(truncated)?;
        let directory_count = read_u32(&data, directory_count_offset).ok_or_else(truncated)?;

        let mut sections = Vec::with_capacity(section_count);
        let section_offset = optional + optional_size;
        for index in 0..section_count {
            let base = section_offset + index * 40;
            sections.push(Section {
                virtual_size: read_u32(&data, base + 8).ok_or_else(truncated)?,
                virtual_address: read_u32(&data, base + 12).ok_or_else(truncated)?,
                raw_size: read_u32(&data, base + 16).ok_or_else(truncated)?,
                raw_pointer: read_u32(&data, base + 20).ok_or_else(truncated)?,
            });
        }

        let mut resources = Vec::new();
        if directory_count > 2 {
            let resource_rva = read_u32(&data, directory_offset + 16).ok_or_else(truncated)?;
            let resource_size = read_u32(&data, directory_offset + 20).ok_or_else(truncated)?;
            if resource_rva != 0 && resource_size != 0 {
                resources = parse_resources(&data, &sections, size_of_headers, resource_rva, path)?;
            }
        }
        resources.sort_by_key(|entry| (entry.type_id, entry.name_id, entry.language_id));

        Ok(Self {
            path: path.to_path_buf(),
            data,
            machine,
            subsystem,
            is_dll: characteristics & 0x2000 != 0,
            is_64bit,
            image_base,
            entry_point,
            resources,
        })
    }

    fn find(&self, type_id: u32, name_id: u32, language: u32) -> Option<&ResourceEntry> {
        self.resources.iter().find(|entry| {
            entry.type_id == type_id && entry.name_id == name_id && entry.language_id == language
        })
    }

    fn resource_data(&self, entry: &ResourceEntry) -> Result<&[u8], RscmdError> {
        let offset = entry
            .data_offset
            .ok_or_else(|| parse_error(&self.path, "资源数据 RVA 无法映射到文件偏移"))?;
        let end = offset + entry.size as usize;
        // 解析阶段已保证 offset + size 不越界。
        Ok(&self.data[offset..end])
    }
}

fn load(path: &Path) -> Result<PeImage, RscmdError> {
    let supported = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            PE_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(ext))
        });
    let data = fs::read(path).map_err(|err| io_error(err, "file", path))?;
    if !supported {
        return Err(parse_error(
            path,
            "扩展名不在支持列表（exe/dll/sys/ocx/scr/cpl/ax/acm/drv）",
        ));
    }
    PeImage::parse(data, path)
}

fn parse_resources(
    data: &[u8],
    sections: &[Section],
    size_of_headers: u32,
    directory_rva: u32,
    path: &Path,
) -> Result<Vec<ResourceEntry>, RscmdError> {
    let base = rva_to_offset(directory_rva, sections, size_of_headers)
        .filter(|offset| *offset < data.len())
        .ok_or_else(|| parse_error(path, "资源目录 RVA 无法映射"))?;
    let mut walker = ResourceWalker {
        data,
        sections,
        size_of_headers,
        base,
        path,
        entries: Vec::new(),
    };
    walker.walk(base, 0, 0, 0)?;
    Ok(walker.entries)
}

struct ResourceWalker<'a> {
    data: &'a [u8],
    sections: &'a [Section],
    size_of_headers: u32,
    base: usize,
    path: &'a Path,
    entries: Vec<ResourceEntry>,
}

impl ResourceWalker<'_> {
    fn walk(
        &mut self,
        directory: usize,
        depth: usize,
        type_id: u32,
        name_id: u32,
    ) -> Result<(), RscmdError> {
        // IMAGE_RESOURCE_DIRECTORY: 16 字节头，条目计数在 +12 / +14。
        let named = self.read_u16(directory + 12, "资源目录头越界")? as usize;
        let ids = self.read_u16(directory + 14, "资源目录头越界")? as usize;
        let total = named + ids;
        if total > MAX_DIRECTORY_ENTRIES {
            return Err(self.error("资源目录项数量异常"));
        }
        let start = directory + 16;
        for index in 0..total {
            let entry = start + index * 8;
            let raw_name = self.read_u32(entry, "资源目录项越界")?;
            if raw_name & 0x8000_0000 != 0 {
                // 命名资源（字符串 ID）与 fcmd 一致跳过。
                continue;
            }
            let raw_offset = self.read_u32(entry + 4, "资源目录项越界")?;
            if depth < 2 {
                if raw_offset & 0x8000_0000 == 0 {
                    continue;
                }
                let child = self.base + (raw_offset & 0x7fff_ffff) as usize;
                let (next_type, next_name) = if depth == 0 {
                    (raw_name, 0)
                } else {
                    (type_id, raw_name)
                };
                self.walk(child, depth + 1, next_type, next_name)?;
                continue;
            }
            if raw_offset & 0x8000_0000 != 0 {
                continue;
            }
            let data_entry = self.base + raw_offset as usize;
            let data_rva = self.read_u32(data_entry, "资源数据项越界")?;
            let size = self.read_u32(data_entry + 4, "资源数据项越界")?;
            if self.entries.len() >= MAX_RESOURCES {
                return Err(self.error("资源数量异常"));
            }
            let data_offset =
                rva_to_offset(data_rva, self.sections, self.size_of_headers).filter(|offset| {
                    offset
                        .checked_add(size as usize)
                        .is_some_and(|end| end <= self.data.len())
                });
            self.entries.push(ResourceEntry {
                type_id,
                name_id,
                language_id: raw_name,
                data_rva,
                size,
                entry_offset: data_entry,
                data_offset,
            });
        }
        Ok(())
    }

    fn read_u16(&self, offset: usize, detail: &'static str) -> Result<u16, RscmdError> {
        read_u16(self.data, offset).ok_or_else(|| self.error(detail))
    }

    fn read_u32(&self, offset: usize, detail: &'static str) -> Result<u32, RscmdError> {
        read_u32(self.data, offset).ok_or_else(|| self.error(detail))
    }

    fn error(&self, detail: &'static str) -> RscmdError {
        parse_error(self.path, detail)
    }
}

fn rva_to_offset(rva: u32, sections: &[Section], size_of_headers: u32) -> Option<usize> {
    if rva < size_of_headers {
        return Some(rva as usize);
    }
    for section in sections {
        let span = section.virtual_size.max(section.raw_size);
        if rva >= section.virtual_address && rva - section.virtual_address < span {
            let delta = rva - section.virtual_address;
            return section
                .raw_pointer
                .checked_add(delta)
                .map(|raw| raw as usize);
        }
    }
    None
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    let bytes = data.get(offset..offset.checked_add(8)?)?;
    Some(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn parse_error(path: &Path, detail: impl Into<String>) -> RscmdError {
    RscmdError::coded(
        "PARSE_FAILED",
        format!("无法解析 PE 文件 {}: {}", path.display(), detail.into()),
    )
    .with_field("file")
    .with_expected("有效的 PE 映像（exe/dll/sys 等）")
}

fn io_error(err: io::Error, field: &str, path: &Path) -> RscmdError {
    RscmdError::coded("IO", format!("文件操作失败: {}: {err}", path.display())).with_field(field)
}

fn resource_not_found(path: &Path, type_id: u32, name_id: u32, language: u32) -> RscmdError {
    RscmdError::coded(
        "RESOURCE_NOT_FOUND",
        format!(
            "{} 中未找到资源 Type={type_id}, Name={name_id}, Language={language}",
            path.display()
        ),
    )
    .with_field("type_id")
    .with_hint("先运行 `rscmd reshack list <FILE>` 查看可用资源")
}

fn machine_name(machine: u16) -> &'static str {
    match machine {
        0x014C => "x86",
        0x0200 => "ia64",
        0x01C0 => "arm",
        0x01C4 => "armv7",
        0xAA64 => "arm64",
        0x8664 => "x64",
        _ => "unknown",
    }
}

fn subsystem_name(subsystem: u16) -> &'static str {
    match subsystem {
        1 => "native",
        2 => "windows_gui",
        3 => "windows_console",
        5 => "os2_console",
        7 => "posix_console",
        9 => "windows_ce_gui",
        10 => "efi_application",
        11 => "efi_boot_service_driver",
        12 => "efi_runtime_driver",
        13 => "efi_rom",
        14 => "xbox",
        16 => "windows_boot_application",
        _ => "unknown",
    }
}

fn resource_type_name(type_id: u32) -> String {
    match type_id {
        1 => "Cursor",
        2 => "Bitmap",
        3 => "Icon",
        4 => "Menu",
        5 => "Dialog",
        6 => "String Table",
        7 => "Font Directory",
        8 => "Font",
        9 => "Accelerator",
        10 => "Raw Data",
        11 => "Message Table",
        12 => "Group Cursor",
        14 => "Group Icon",
        16 => "Version",
        17 => "Dialog Include",
        19 => "Plug and Play",
        20 => "VXD",
        21 => "Animated Cursor",
        22 => "Animated Icon",
        23 => "HTML",
        24 => "Manifest",
        other => return format!("Unknown ({other})"),
    }
    .to_string()
}

fn resource_extension(type_id: u32) -> &'static str {
    match type_id {
        1 => ".cur",
        2 => ".bmp",
        3 => ".ico",
        4 => ".rc",
        5 => ".rc",
        6 => ".txt",
        9 => ".rc",
        10 => ".bin",
        11 => ".txt",
        12 => ".cur",
        14 => ".ico",
        16 => ".txt",
        21 => ".ani",
        22 => ".ani",
        23 => ".html",
        24 => ".xml",
        _ => ".bin",
    }
}

fn bitmap_to_bmp(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 40 {
        return None;
    }
    let header_size = read_u32(data, 0)?;
    if header_size < 40 {
        return None;
    }
    let bit_count = read_u16(data, 14)?;
    let colors_used = read_u32(data, 32)?;
    let palette = if colors_used != 0 {
        colors_used
    } else if bit_count <= 8 {
        1u32 << bit_count
    } else {
        0
    };
    let offset = 14u32
        .checked_add(header_size)?
        .checked_add(palette.checked_mul(4)?)?;
    let mut out = Vec::with_capacity(14 + data.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(14u32 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(data);
    Some(out)
}

fn icon_to_ico(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 40 {
        return None;
    }
    let width = read_u32(data, 4)? as i32;
    let height = read_u32(data, 8)? as i32 / 2;
    let bit_count = read_u16(data, 14)?;
    if width <= 0 || height <= 0 {
        return None;
    }
    let entry_width = if width >= 256 { 0 } else { width as u8 };
    let entry_height = if height >= 256 { 0 } else { height as u8 };
    let mut out = Vec::with_capacity(22 + data.len());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&[entry_width, entry_height, 0, 0]);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&bit_count.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes());
    out.extend_from_slice(data);
    Some(out)
}

fn export_payload(type_id: u32, data: &[u8]) -> Vec<u8> {
    match type_id {
        RT_ICON => icon_to_ico(data).unwrap_or_else(|| data.to_vec()),
        RT_BITMAP => bitmap_to_bmp(data).unwrap_or_else(|| data.to_vec()),
        _ => data.to_vec(),
    }
}

fn default_export_path(file: &Path, type_id: u32, name_id: u32, language: u32) -> PathBuf {
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("resource");
    PathBuf::from(format!(
        "{stem}_resource_{type_id}_{name_id}_{language}{}",
        resource_extension(type_id)
    ))
}

fn export_all_root(file: &Path) -> PathBuf {
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("resource");
    let parent = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    parent.join(format!("{stem}_resources"))
}

fn backup_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "resource".to_string());
    let mut candidate = path.with_file_name(format!("{name}.backup"));
    let mut counter = 1usize;
    while candidate.exists() {
        candidate = path.with_file_name(format!("{name}.backup{counter}"));
        counter += 1;
    }
    candidate
}

fn list(image: &PeImage, file: &str) -> Value {
    let types: Vec<String> = image
        .resources
        .iter()
        .map(|entry| resource_type_name(entry.type_id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let resources: Vec<Value> = image
        .resources
        .iter()
        .map(|entry| {
            json!({
                "type_id": entry.type_id,
                "type_name": resource_type_name(entry.type_id),
                "name_id": entry.name_id,
                "language_id": entry.language_id,
                "rva": entry.data_rva,
                "size": entry.size,
            })
        })
        .collect();
    json!({
        "op": "list",
        "file": file,
        "info": {
            "machine": machine_name(image.machine),
            "machine_raw": image.machine,
            "subsystem": subsystem_name(image.subsystem),
            "subsystem_raw": image.subsystem,
            "is_dll": image.is_dll,
            "is_64bit": image.is_64bit,
            "image_base": image.image_base,
            "entry_point": image.entry_point,
        },
        "resource_count": resources.len(),
        "types": types,
        "resources": resources,
    })
}

fn extract(ctx: &mut ToolCtx, input: &ReshackInput) -> Result<Value, RscmdError> {
    let file = Path::new(&input.file);
    let image = load(file)?;
    let type_id = require_id(input.type_id, "type_id", "extract 需要资源类型 ID")?;
    let name_id = require_id(input.name_id, "name_id", "extract 需要资源名称 ID")?;
    let entry = image
        .find(type_id, name_id, input.language)
        .ok_or_else(|| resource_not_found(file, type_id, name_id, input.language))?;
    let payload = export_payload(type_id, image.resource_data(entry)?);
    let target = input
        .output
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_export_path(file, type_id, name_id, input.language));

    ctx.require_confirm("导出 PE 资源到文件")?;
    let dry_run = ctx.dry_run;
    if !dry_run {
        atomic_write(&target, &payload).map_err(|err| io_error(err, "output", &target))?;
    }

    Ok(json!({
        "op": "extract",
        "file": input.file,
        "type_id": type_id,
        "type_name": resource_type_name(type_id),
        "name_id": name_id,
        "language_id": input.language,
        "output": target.to_string_lossy().into_owned(),
        "resource_size": entry.size,
        "output_bytes": payload.len(),
        "dry_run": dry_run,
    }))
}

fn export_all(ctx: &mut ToolCtx, input: &ReshackInput) -> Result<Value, RscmdError> {
    let file = Path::new(&input.file);
    let image = load(file)?;
    let root = input
        .output_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| export_all_root(file));

    let planned: Vec<(PathBuf, Vec<u8>)> = image
        .resources
        .iter()
        .map(|entry| {
            let target = root.join(resource_type_name(entry.type_id)).join(format!(
                "{}_{}{}",
                entry.name_id,
                entry.language_id,
                resource_extension(entry.type_id)
            ));
            let payload = export_payload(entry.type_id, image.resource_data(entry)?);
            Ok((target, payload))
        })
        .collect::<Result<_, RscmdError>>()?;

    ctx.require_confirm("导出全部 PE 资源到目录")?;
    let dry_run = ctx.dry_run;
    let mut exported = 0usize;
    if !dry_run {
        for (target, payload) in &planned {
            atomic_write(target, payload).map_err(|err| io_error(err, "output", target))?;
            exported += 1;
        }
    }

    let mut out = json!({
        "op": "export-all",
        "file": input.file,
        "output_dir": root.to_string_lossy().into_owned(),
        "resource_count": planned.len(),
        "exported": exported,
        "dry_run": dry_run,
    });
    if dry_run {
        let limit = ctx.limit.filter(|limit| *limit > 0);
        let mut files: Vec<String> = planned
            .iter()
            .map(|(target, _)| target.to_string_lossy().into_owned())
            .collect();
        if let Some(limit) = limit {
            files.truncate(limit);
        }
        out["files"] = json!(files);
        out["truncated"] = json!(limit.is_some_and(|limit| limit < planned.len()));
    }
    Ok(out)
}

fn replace(ctx: &mut ToolCtx, input: &ReshackInput) -> Result<Value, RscmdError> {
    let file = Path::new(&input.file);
    let image = load(file)?;
    let type_id = require_id(input.type_id, "type_id", "replace 需要资源类型 ID")?;
    let name_id = require_id(input.name_id, "name_id", "replace 需要资源名称 ID")?;
    let input_path = PathBuf::from(input.input.as_deref().ok_or_else(|| {
        RscmdError::coded("INVALID_ARGS", "replace 需要新资源数据文件").with_field("input")
    })?);
    let new_data = fs::read(&input_path).map_err(|err| io_error(err, "input", &input_path))?;
    let entry = image
        .find(type_id, name_id, input.language)
        .ok_or_else(|| resource_not_found(file, type_id, name_id, input.language))?;
    let old_size = entry.size as usize;
    if new_data.len() > old_size {
        return Err(RscmdError::coded(
            "INVALID_ARGS",
            format!(
                "新数据 {} 字节大于原资源 {old_size} 字节，PE 资源只能原位覆盖",
                new_data.len()
            ),
        )
        .with_field("input")
        .with_expected("不大于原资源的字节数"));
    }

    let backup = backup_path(file);
    ctx.require_confirm("替换 PE 资源（写入前自动备份）")?;
    let dry_run = ctx.dry_run;
    if !dry_run {
        fs::copy(file, &backup).map_err(|err| io_error(err, "file", file))?;
        let offset = entry
            .data_offset
            .ok_or_else(|| parse_error(file, "资源数据 RVA 无法映射到文件偏移"))?;
        let mut data = image.data.clone();
        data[offset..offset + old_size].fill(0);
        data[offset..offset + new_data.len()].copy_from_slice(&new_data);
        data[entry.entry_offset + 4..entry.entry_offset + 8]
            .copy_from_slice(&(new_data.len() as u32).to_le_bytes());
        if let Err(err) = atomic_write(file, &data) {
            let _ = fs::remove_file(&backup);
            return Err(io_error(err, "file", file));
        }
    }

    Ok(json!({
        "op": "replace",
        "file": input.file,
        "type_id": type_id,
        "type_name": resource_type_name(type_id),
        "name_id": name_id,
        "language_id": input.language,
        "old_size": old_size,
        "new_size": new_data.len(),
        "backup": backup.to_string_lossy().into_owned(),
        "dry_run": dry_run,
    }))
}

fn require_id(value: Option<u32>, field: &str, message: &str) -> Result<u32, RscmdError> {
    value.ok_or_else(|| RscmdError::coded("INVALID_ARGS", message).with_field(field))
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(reshack_args_schema),
    }
}

fn reshack_args_schema() -> String {
    let schema = schemars::schema_for!(ReshackInput);
    serde_json::to_string_pretty(&schema).expect("ReshackInput schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rscmd_runtime::fs::temp_dir;

    /// 资源目录在文件中的起始偏移（对应 RVA 0x1000）。
    const RES_OFFSET: usize = 0x200;
    const MANIFEST: &[u8] = br#"<?xml version="1.0"?><assembly/>"#;

    fn put_u16(buffer: &mut [u8], offset: usize, value: u16) {
        buffer[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(buffer: &mut [u8], offset: usize, value: u32) {
        buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u64(buffer: &mut [u8], offset: usize, value: u64) {
        buffer[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    /// 构造一个 40 字节的 DIB 头，并在尾部补 24 字节模拟像素数据。
    fn dib_header(width: i32, height: i32, bit_count: u16) -> Vec<u8> {
        let mut data = vec![0u8; 64];
        put_u32(&mut data, 0, 40);
        put_u32(&mut data, 4, width as u32);
        put_u32(&mut data, 8, height as u32);
        put_u16(&mut data, 12, 1);
        put_u16(&mut data, 14, bit_count);
        data
    }

    /// 构造含 Icon 与 Manifest 各一份的 PE32+ 文件；根目录带一个命名项用于验证跳过逻辑。
    fn sample_pe() -> Vec<u8> {
        let icon = dib_header(16, 32, 32);
        let resource_bytes = 0x100 + icon.len() + MANIFEST.len();
        let mut data = vec![0u8; RES_OFFSET + resource_bytes];

        data[0..2].copy_from_slice(b"MZ");
        put_u32(&mut data, 0x3C, 0x40);
        data[0x40..0x44].copy_from_slice(b"PE\0\0");
        put_u16(&mut data, 0x44, 0x8664);
        put_u16(&mut data, 0x46, 1);
        put_u16(&mut data, 0x54, 0xF0);
        put_u16(&mut data, 0x56, 0x0022);

        let optional = 0x58;
        put_u16(&mut data, optional, 0x20B);
        put_u32(&mut data, optional + 16, 0x1234);
        put_u64(&mut data, optional + 24, 0x1_4000_0000);
        put_u32(&mut data, optional + 56, 0x2000);
        put_u32(&mut data, optional + 60, 0x200);
        put_u16(&mut data, optional + 68, 2);
        put_u32(&mut data, optional + 108, 16);
        put_u32(&mut data, optional + 112 + 16, 0x1000);
        put_u32(&mut data, optional + 112 + 20, resource_bytes as u32);

        let section = optional + 0xF0;
        data[section..section + 5].copy_from_slice(b".rsrc");
        put_u32(&mut data, section + 8, resource_bytes as u32);
        put_u32(&mut data, section + 12, 0x1000);
        put_u32(&mut data, section + 16, resource_bytes as u32);
        put_u32(&mut data, section + 20, RES_OFFSET as u32);

        let base = RES_OFFSET;
        // 根目录：1 个命名项 + 2 个数字类型项。
        put_u16(&mut data, base + 12, 1);
        put_u16(&mut data, base + 14, 2);
        put_u32(&mut data, base + 16, 0x8000_0000);
        put_u32(&mut data, base + 20, 0x8000_0000);
        put_u32(&mut data, base + 24, RT_ICON);
        put_u32(&mut data, base + 28, 0x8000_0028);
        put_u32(&mut data, base + 32, 24);
        put_u32(&mut data, base + 36, 0x8000_0068);

        // 类型 3 → 名称 1 → 语言 0。
        put_u16(&mut data, base + 0x34, 0);
        put_u16(&mut data, base + 0x36, 1);
        put_u32(&mut data, base + 0x38, 1);
        put_u32(&mut data, base + 0x3C, 0x8000_0040);
        put_u16(&mut data, base + 0x4C, 0);
        put_u16(&mut data, base + 0x4E, 1);
        put_u32(&mut data, base + 0x50, 0);
        put_u32(&mut data, base + 0x54, 0x58);
        put_u32(&mut data, base + 0x58, 0x1100);
        put_u32(&mut data, base + 0x5C, icon.len() as u32);

        // 类型 24 → 名称 1 → 语言 1033。
        put_u16(&mut data, base + 0x74, 0);
        put_u16(&mut data, base + 0x76, 1);
        put_u32(&mut data, base + 0x78, 1);
        put_u32(&mut data, base + 0x7C, 0x8000_0080);
        put_u16(&mut data, base + 0x8C, 0);
        put_u16(&mut data, base + 0x8E, 1);
        put_u32(&mut data, base + 0x90, 1033);
        put_u32(&mut data, base + 0x94, 0x98);
        put_u32(&mut data, base + 0x98, 0x1100 + icon.len() as u32);
        put_u32(&mut data, base + 0x9C, MANIFEST.len() as u32);

        data[base + 0x100..base + 0x100 + icon.len()].copy_from_slice(&icon);
        let manifest_start = base + 0x100 + icon.len();
        data[manifest_start..manifest_start + MANIFEST.len()].copy_from_slice(MANIFEST);
        data
    }

    /// 构造不含资源的 PE32 文件（同时覆盖 32 位可选头分支）。
    fn empty_pe32() -> Vec<u8> {
        let mut data = vec![0u8; 0x200];
        data[0..2].copy_from_slice(b"MZ");
        put_u32(&mut data, 0x3C, 0x40);
        data[0x40..0x44].copy_from_slice(b"PE\0\0");
        put_u16(&mut data, 0x44, 0x014C);
        put_u16(&mut data, 0x54, 0xE0);
        put_u16(&mut data, 0x56, 0x2102);
        let optional = 0x58;
        put_u16(&mut data, optional, 0x10B);
        put_u32(&mut data, optional + 28, 0x40_0000);
        put_u32(&mut data, optional + 60, 0x200);
        put_u16(&mut data, optional + 68, 3);
        put_u32(&mut data, optional + 92, 16);
        data
    }

    fn fixture(dir: &rscmd_runtime::fs::TempDir, bytes: &[u8]) -> String {
        let path = dir.path().join("app.exe");
        fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn confirmed() -> ToolCtx {
        ToolCtx {
            confirm: true,
            ..ToolCtx::default()
        }
    }

    #[test]
    fn lists_header_and_resources_skipping_named_entries() {
        let dir = temp_dir().unwrap();
        let file = fixture(&dir, &sample_pe());
        let out = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "list", "file": file}),
        )
        .unwrap();
        assert_eq!(out["resource_count"], 2);
        assert_eq!(out["types"], json!(["Icon", "Manifest"]));
        assert_eq!(out["resources"][0]["type_id"], 3);
        assert_eq!(out["resources"][0]["type_name"], "Icon");
        assert_eq!(out["resources"][0]["name_id"], 1);
        assert_eq!(out["resources"][0]["language_id"], 0);
        assert_eq!(out["resources"][0]["rva"], 0x1100u32);
        assert_eq!(out["resources"][0]["size"], 64);
        assert_eq!(out["resources"][1]["type_id"], 24);
        assert_eq!(out["resources"][1]["language_id"], 1033);
        assert_eq!(out["info"]["machine"], "x64");
        assert_eq!(out["info"]["machine_raw"], 0x8664);
        assert_eq!(out["info"]["subsystem"], "windows_gui");
        assert_eq!(out["info"]["subsystem_raw"], 2);
        assert_eq!(out["info"]["is_dll"], false);
        assert_eq!(out["info"]["is_64bit"], true);
        assert_eq!(out["info"]["image_base"], 0x1_4000_0000u64);
        assert_eq!(out["info"]["entry_point"], 0x1234);
    }

    #[test]
    fn lists_pe32_without_resources() {
        let dir = temp_dir().unwrap();
        let file = fixture(&dir, &empty_pe32());
        let out = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "list", "file": file}),
        )
        .unwrap();
        assert_eq!(out["resource_count"], 0);
        assert_eq!(out["types"], json!([]));
        assert_eq!(out["info"]["machine"], "x86");
        assert_eq!(out["info"]["subsystem"], "windows_console");
        assert_eq!(out["info"]["is_dll"], true);
        assert_eq!(out["info"]["is_64bit"], false);
        assert_eq!(out["info"]["image_base"], 0x40_0000);
        assert_eq!(out["info"]["entry_point"], 0);
    }

    #[test]
    fn extract_wraps_icon_and_requires_confirm() {
        let dir = temp_dir().unwrap();
        let file = fixture(&dir, &sample_pe());
        let target = dir.path().join("icon.ico");
        let input = json!({
            "op": "extract",
            "file": file,
            "type_id": 3,
            "name_id": 1,
            "output": target.to_string_lossy(),
        });

        let err = run_json(&mut ToolCtx::default(), &input).unwrap_err();
        assert_eq!(err.code, "CONFIRM_REQUIRED");
        assert!(!target.exists());

        let out = run_json(&mut confirmed(), &input).unwrap();
        assert_eq!(out["resource_size"], 64);
        assert_eq!(out["output_bytes"], 86);
        assert_eq!(out["dry_run"], false);
        let bytes = fs::read(&target).unwrap();
        assert_eq!(bytes.len(), 86);
        assert_eq!(&bytes[..6], &[0, 0, 1, 0, 1, 0]);
    }

    #[test]
    fn extract_manifest_raw_and_default_name() {
        let dir = temp_dir().unwrap();
        let file = fixture(&dir, &sample_pe());
        let target = dir.path().join("app.manifest");
        let out = run_json(
            &mut confirmed(),
            &json!({
                "op": "extract",
                "file": file,
                "type_id": 24,
                "name_id": 1,
                "language": 1033,
                "output": target.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(out["type_name"], "Manifest");
        assert_eq!(fs::read(&target).unwrap(), MANIFEST);

        let mut dry = ToolCtx {
            dry_run: true,
            ..ToolCtx::default()
        };
        let out = run_json(
            &mut dry,
            &json!({"op": "extract", "file": file, "type_id": 3, "name_id": 1}),
        )
        .unwrap();
        assert_eq!(out["dry_run"], true);
        assert_eq!(out["output"], "app_resource_3_1_0.ico");
    }

    #[test]
    fn extract_missing_resource_and_unmapped_data() {
        let dir = temp_dir().unwrap();
        let file = fixture(&dir, &sample_pe());
        let err = run_json(
            &mut confirmed(),
            &json!({"op": "extract", "file": file, "type_id": 99, "name_id": 1}),
        )
        .unwrap_err();
        assert_eq!(err.code, "RESOURCE_NOT_FOUND");
        assert_eq!(err.exit_code(), 4);

        let mut broken = sample_pe();
        put_u32(&mut broken, RES_OFFSET + 0x58, 0x9999_0000);
        let file = fixture(&dir, &broken);
        let err = run_json(
            &mut confirmed(),
            &json!({"op": "extract", "file": file, "type_id": 3, "name_id": 1, "output": "x.ico"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
    }

    #[test]
    fn export_all_writes_type_directories() {
        let dir = temp_dir().unwrap();
        let file = fixture(&dir, &sample_pe());
        let root = dir.path().join("out");
        let out = run_json(
            &mut confirmed(),
            &json!({"op": "export-all", "file": file, "output_dir": root.to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(out["resource_count"], 2);
        assert_eq!(out["exported"], 2);
        assert_eq!(out["dry_run"], false);
        assert_eq!(
            fs::read(root.join("Manifest/1_1033.xml")).unwrap(),
            MANIFEST
        );
        let icon = fs::read(root.join("Icon/1_0.ico")).unwrap();
        assert_eq!(icon.len(), 86);
        assert_eq!(&icon[..6], &[0, 0, 1, 0, 1, 0]);
    }

    #[test]
    fn export_all_dry_run_respects_limit_and_requires_confirm() {
        let dir = temp_dir().unwrap();
        let file = fixture(&dir, &sample_pe());
        let root = dir.path().join("out");
        let input = json!({"op": "export-all", "file": file, "output_dir": root.to_string_lossy()});

        let err = run_json(&mut ToolCtx::default(), &input).unwrap_err();
        assert_eq!(err.code, "CONFIRM_REQUIRED");

        let mut dry = ToolCtx {
            dry_run: true,
            limit: Some(1),
            ..ToolCtx::default()
        };
        let out = run_json(&mut dry, &input).unwrap();
        assert_eq!(out["exported"], 0);
        assert_eq!(out["files"].as_array().unwrap().len(), 1);
        assert_eq!(out["truncated"], true);
        assert!(!root.exists());
    }

    #[test]
    fn replace_pads_zero_updates_size_and_backs_up() {
        let dir = temp_dir().unwrap();
        let original = sample_pe();
        let file = fixture(&dir, &original);
        let data = dir.path().join("new.bin");
        fs::write(&data, [0xAB; 10]).unwrap();
        let input = json!({
            "op": "replace",
            "file": file,
            "type_id": 3,
            "name_id": 1,
            "input": data.to_string_lossy(),
        });

        let err = run_json(&mut ToolCtx::default(), &input).unwrap_err();
        assert_eq!(err.code, "CONFIRM_REQUIRED");
        assert_eq!(fs::read(dir.path().join("app.exe")).unwrap(), original);

        let out = run_json(&mut confirmed(), &input).unwrap();
        assert_eq!(out["old_size"], 64);
        assert_eq!(out["new_size"], 10);
        let backup = dir.path().join("app.exe.backup");
        assert_eq!(out["backup"], backup.to_string_lossy().into_owned());
        assert_eq!(fs::read(&backup).unwrap(), original);

        let modified = fs::read(dir.path().join("app.exe")).unwrap();
        let icon_offset = RES_OFFSET + 0x100;
        assert_eq!(&modified[icon_offset..icon_offset + 10], &[0xAB; 10]);
        assert!(modified[icon_offset + 10..icon_offset + 64]
            .iter()
            .all(|byte| *byte == 0));

        let image = load(Path::new(&file)).unwrap();
        let entry = image.find(3, 1, 0).unwrap();
        assert_eq!(entry.size, 10);
        assert_eq!(image.resource_data(entry).unwrap(), &[0xAB; 10]);
    }

    #[test]
    fn replace_dry_run_and_numbered_backup() {
        let dir = temp_dir().unwrap();
        let original = sample_pe();
        let file = fixture(&dir, &original);
        let data = dir.path().join("new.bin");
        fs::write(&data, [1u8; 4]).unwrap();
        let input = json!({
            "op": "replace",
            "file": file,
            "type_id": 3,
            "name_id": 1,
            "input": data.to_string_lossy(),
        });

        let mut dry = ToolCtx {
            dry_run: true,
            ..ToolCtx::default()
        };
        let out = run_json(&mut dry, &input).unwrap();
        assert_eq!(out["dry_run"], true);
        assert!(out["backup"].as_str().unwrap().ends_with("app.exe.backup"));
        assert_eq!(fs::read(dir.path().join("app.exe")).unwrap(), original);
        assert!(!dir.path().join("app.exe.backup").exists());

        fs::write(dir.path().join("app.exe.backup"), b"old").unwrap();
        let out = run_json(&mut confirmed(), &input).unwrap();
        assert!(out["backup"].as_str().unwrap().ends_with("app.exe.backup1"));
    }

    #[test]
    fn replace_rejects_oversized_payload_without_touching_file() {
        let dir = temp_dir().unwrap();
        let original = sample_pe();
        let file = fixture(&dir, &original);
        let data = dir.path().join("big.bin");
        fs::write(&data, [0u8; 65]).unwrap();
        let err = run_json(
            &mut confirmed(),
            &json!({
                "op": "replace",
                "file": file,
                "type_id": 3,
                "name_id": 1,
                "input": data.to_string_lossy(),
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));
        assert_eq!(err.exit_code(), 3);
        assert_eq!(fs::read(dir.path().join("app.exe")).unwrap(), original);
        assert!(!dir.path().join("app.exe.backup").exists());
    }

    #[test]
    fn replace_requires_input_file_and_existing_resource() {
        let dir = temp_dir().unwrap();
        let file = fixture(&dir, &sample_pe());
        let err = run_json(
            &mut confirmed(),
            &json!({"op": "replace", "file": file, "type_id": 3, "name_id": 1}),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));

        let data = dir.path().join("new.bin");
        fs::write(&data, [1u8; 4]).unwrap();
        let err = run_json(
            &mut confirmed(),
            &json!({
                "op": "replace",
                "file": file,
                "type_id": 99,
                "name_id": 1,
                "input": data.to_string_lossy(),
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "RESOURCE_NOT_FOUND");

        let missing = dir.path().join("missing.bin");
        let err = run_json(
            &mut confirmed(),
            &json!({
                "op": "replace",
                "file": file,
                "type_id": 3,
                "name_id": 1,
                "input": missing.to_string_lossy(),
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.field.as_deref(), Some("input"));
    }

    #[test]
    fn rejects_non_pe_inputs() {
        let dir = temp_dir().unwrap();

        let txt = dir.path().join("app.txt");
        fs::write(&txt, sample_pe()).unwrap();
        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "list", "file": txt.to_string_lossy()}),
        )
        .unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
        assert!(err.message.contains("扩展名"));

        let short = dir.path().join("short.exe");
        fs::write(&short, b"not a pe").unwrap();
        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "list", "file": short.to_string_lossy()}),
        )
        .unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");

        let missing = dir.path().join("missing.exe");
        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "list", "file": missing.to_string_lossy()}),
        )
        .unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn rejects_pe_with_bad_signature_or_magic() {
        let dir = temp_dir().unwrap();

        let mut bad_signature = sample_pe();
        bad_signature[0x40..0x44].copy_from_slice(b"XXXX");
        let file = fixture(&dir, &bad_signature);
        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "list", "file": file}),
        )
        .unwrap_err();
        assert!(err.message.contains("签名"));

        let mut bad_magic = sample_pe();
        put_u16(&mut bad_magic, 0x58, 0x999);
        let file = fixture(&dir, &bad_magic);
        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "list", "file": file}),
        )
        .unwrap_err();
        assert!(err.message.contains("Magic"));

        let mut bad_directory = sample_pe();
        put_u32(&mut bad_directory, 0x58 + 112 + 16, 0x9999_0000);
        let file = fixture(&dir, &bad_directory);
        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "list", "file": file}),
        )
        .unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
        assert!(err.message.contains("资源目录"));
    }

    #[test]
    fn json_rejects_unknown_op_and_missing_fields() {
        let mut ctx = ToolCtx::default();
        let err = run_json(&mut ctx, &json!({"op": "nope", "file": "app.exe"})).unwrap_err();
        assert_eq!(err.code, "INVALID_MODE");
        assert_eq!(err.field.as_deref(), Some("op"));

        let dir = temp_dir().unwrap();
        let file = fixture(&dir, &sample_pe());
        let err = run_json(&mut ctx, &json!({"op": "extract", "file": file})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("type_id"));

        let err = run_json(
            &mut ctx,
            &json!({"op": "extract", "file": file, "type_id": 3}),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("name_id"));

        let err = run_json(&mut ctx, &json!({"op": "list"})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn cli_parses_all_subcommands() {
        let dir = temp_dir().unwrap();
        let file = fixture(&dir, &sample_pe());
        let mut ctx = ToolCtx {
            dry_run: true,
            ..ToolCtx::default()
        };

        let matches = build()
            .try_get_matches_from(["reshack", "list", &file])
            .unwrap();
        assert_eq!(run_cli(&mut ctx, &matches).unwrap()["resource_count"], 2);

        let matches = build()
            .try_get_matches_from(["reshack", "extract", &file, "3", "1", "-o", "icon.ico"])
            .unwrap();
        let out = run_cli(&mut ctx, &matches).unwrap();
        assert_eq!(out["dry_run"], true);
        assert_eq!(out["output"], "icon.ico");

        let data = dir.path().join("new.bin");
        fs::write(&data, [1u8; 4]).unwrap();
        let data = data.to_string_lossy().into_owned();
        let matches = build()
            .try_get_matches_from(["reshack", "replace", &file, "3", "1", &data])
            .unwrap();
        assert_eq!(run_cli(&mut ctx, &matches).unwrap()["new_size"], 4);

        let out_dir = dir.path().join("out").to_string_lossy().into_owned();
        let matches = build()
            .try_get_matches_from(["reshack", "export-all", &file, "-d", &out_dir])
            .unwrap();
        assert_eq!(run_cli(&mut ctx, &matches).unwrap()["resource_count"], 2);

        let matches = Command::new("reshack")
            .try_get_matches_from(["reshack"])
            .unwrap();
        let err = run_cli(&mut ctx, &matches).unwrap_err();
        assert_eq!(err.code, "INVALID_MODE");
    }

    #[test]
    fn maps_machine_subsystem_type_and_extension() {
        assert_eq!(machine_name(0x014C), "x86");
        assert_eq!(machine_name(0x0200), "ia64");
        assert_eq!(machine_name(0x01C0), "arm");
        assert_eq!(machine_name(0x01C4), "armv7");
        assert_eq!(machine_name(0xAA64), "arm64");
        assert_eq!(machine_name(0x8664), "x64");
        assert_eq!(machine_name(0x9999), "unknown");

        assert_eq!(subsystem_name(1), "native");
        assert_eq!(subsystem_name(2), "windows_gui");
        assert_eq!(subsystem_name(3), "windows_console");
        assert_eq!(subsystem_name(5), "os2_console");
        assert_eq!(subsystem_name(7), "posix_console");
        assert_eq!(subsystem_name(9), "windows_ce_gui");
        assert_eq!(subsystem_name(10), "efi_application");
        assert_eq!(subsystem_name(11), "efi_boot_service_driver");
        assert_eq!(subsystem_name(12), "efi_runtime_driver");
        assert_eq!(subsystem_name(13), "efi_rom");
        assert_eq!(subsystem_name(14), "xbox");
        assert_eq!(subsystem_name(16), "windows_boot_application");
        assert_eq!(subsystem_name(0), "unknown");

        assert_eq!(resource_type_name(24), "Manifest");
        assert_eq!(resource_type_name(6), "String Table");
        assert_eq!(resource_type_name(999), "Unknown (999)");
        assert_eq!(resource_extension(3), ".ico");
        assert_eq!(resource_extension(24), ".xml");
        assert_eq!(resource_extension(999), ".bin");
    }

    #[test]
    fn wraps_icons_and_bitmaps_and_falls_back() {
        let icon = dib_header(16, 32, 32);
        let ico = icon_to_ico(&icon).unwrap();
        assert_eq!(ico.len(), 22 + icon.len());
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!(ico[6], 16);
        assert_eq!(ico[7], 16);
        assert_eq!(u16::from_le_bytes([ico[12], ico[13]]), 32);

        let large = dib_header(256, 512, 32);
        let ico = icon_to_ico(&large).unwrap();
        assert_eq!(ico[6], 0);
        assert_eq!(ico[7], 0);
        assert!(icon_to_ico(&[0u8; 10]).is_none());
        assert!(icon_to_ico(&dib_header(0, 32, 32)).is_none());

        let bitmap = dib_header(2, 2, 24);
        let bmp = bitmap_to_bmp(&bitmap).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(bmp.len(), 14 + bitmap.len());
        assert_eq!(u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]), 54);
        assert!(bitmap_to_bmp(&[0u8; 10]).is_none());
        let mut short_header = dib_header(2, 2, 24);
        put_u32(&mut short_header, 0, 20);
        assert!(bitmap_to_bmp(&short_header).is_none());

        let mut indexed = dib_header(2, 2, 8);
        put_u32(&mut indexed, 32, 2);
        let bmp = bitmap_to_bmp(&indexed).unwrap();
        assert_eq!(
            u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]),
            14 + 40 + 8
        );

        assert_eq!(export_payload(3, &[0u8; 10]), vec![0u8; 10]);
        assert_eq!(export_payload(2, &[0u8; 10]), vec![0u8; 10]);
        assert_eq!(export_payload(16, &[7u8; 4]), vec![7u8; 4]);
    }

    #[test]
    fn default_paths_and_backup_numbering() {
        assert_eq!(
            default_export_path(Path::new("bin/app.exe"), 3, 1, 0),
            PathBuf::from("app_resource_3_1_0.ico")
        );
        assert_eq!(
            export_all_root(Path::new("bin/app.exe")),
            PathBuf::from("bin/app_resources")
        );
        assert_eq!(
            export_all_root(Path::new("app.exe")),
            PathBuf::from(".").join("app_resources")
        );

        let dir = temp_dir().unwrap();
        let file = dir.path().join("app.exe");
        fs::write(&file, b"x").unwrap();
        assert_eq!(backup_path(&file), dir.path().join("app.exe.backup"));
        fs::write(dir.path().join("app.exe.backup"), b"x").unwrap();
        assert_eq!(backup_path(&file), dir.path().join("app.exe.backup1"));
    }

    #[test]
    fn rva_mapping_handles_headers_sections_and_gaps() {
        let sections = vec![Section {
            virtual_address: 0x1000,
            virtual_size: 0x100,
            raw_size: 0x200,
            raw_pointer: 0x400,
        }];
        assert_eq!(rva_to_offset(0x10, &sections, 0x200), Some(0x10));
        assert_eq!(rva_to_offset(0x1100, &sections, 0x200), Some(0x500));
        assert_eq!(rva_to_offset(0x9000, &sections, 0x200), None);
    }

    #[test]
    fn rejects_directory_input() {
        let dir = temp_dir().unwrap();
        let path = dir.path().join("folder.exe");
        fs::create_dir(&path).unwrap();
        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "list", "file": path.to_string_lossy()}),
        )
        .unwrap_err();
        assert_eq!(err.code, "IO");
    }
}
