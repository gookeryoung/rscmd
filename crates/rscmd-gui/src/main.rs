//! reshack GUI POC：验证 egui/eframe、中文字体、ICO/BMP 解码与剪贴板。
//!
//! 默认启动窗口；`--self-test` 只跑无窗口自检，便于在终端验证 POC 的关键能力。
//! `--open <图片>` 可追加 ICO/BMP/PNG 文件（PE 资源请先用 `rscmd reshack extract` 导出）。

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--self-test") {
        match self_test(&args) {
            Ok(lines) => {
                for line in lines {
                    println!("{line}");
                }
                println!("POC self-test: OK");
                Ok(())
            }
            Err(error) => {
                eprintln!("POC self-test: FAILED: {error}");
                std::process::exit(1);
            }
        }
    } else {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1080.0, 720.0])
                .with_min_inner_size([900.0, 560.0]),
            ..Default::default()
        };
        eframe::run_native(
            "reshackw POC",
            options,
            Box::new(|cc| Ok(Box::new(PocApp::new(cc, &args)))),
        )
    }
}

struct PocImage {
    name: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    texture: Option<egui::TextureHandle>,
}

impl PocImage {
    fn from_bytes(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let format = image::guess_format(bytes).map_err(|error| error.to_string())?;
        let decoded = image::load_from_memory_with_format(bytes, format)
            .map_err(|error| error.to_string())?
            .to_rgba8();
        let (width, height) = decoded.dimensions();
        Ok(Self {
            name: name.to_string(),
            width,
            height,
            rgba: decoded.into_raw(),
            texture: None,
        })
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if self.texture.is_none() {
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [self.width as usize, self.height as usize],
                &self.rgba,
            );
            self.texture =
                Some(ctx.load_texture(self.name.clone(), color, egui::TextureOptions::LINEAR));
        }
    }
}

struct PocApp {
    images: Vec<PocImage>,
    selected: usize,
    zoom: f32,
    fit: bool,
    font_status: String,
    clipboard_status: String,
    show_about: bool,
}

impl PocApp {
    fn new(cc: &eframe::CreationContext<'_>, args: &[String]) -> Self {
        let font_status = match install_cjk_font(&cc.egui_ctx) {
            Some(path) => format!("中文字体：{path}"),
            None => "未找到中文字体，中文可能显示为方块".to_string(),
        };
        let mut images = Vec::new();
        for (name, bytes) in sample_inputs(args) {
            match PocImage::from_bytes(&name, &bytes) {
                Ok(image) => images.push(image),
                Err(error) => eprintln!("跳过 {name}：{error}"),
            }
        }
        Self {
            images,
            selected: 0,
            zoom: 1.0,
            fit: true,
            font_status,
            clipboard_status: String::new(),
            show_about: false,
        }
    }
}

impl eframe::App for PocApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (zoom_in_pressed, zoom_out_pressed, reset_pressed) = ctx.input(|input| {
            (
                input.modifiers.command && input.key_pressed(egui::Key::Equals),
                input.modifiers.command && input.key_pressed(egui::Key::Minus),
                input.modifiers.command && input.key_pressed(egui::Key::Num0),
            )
        });
        if zoom_in_pressed {
            self.fit = false;
            self.zoom = rscmd_gui::zoom_in(self.zoom);
        }
        if zoom_out_pressed {
            self.fit = false;
            self.zoom = rscmd_gui::zoom_out(self.zoom);
        }
        if reset_pressed {
            self.fit = false;
            self.zoom = 1.0;
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("文件", |ui| {
                    if ui.button("退出").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("资源", |ui| {
                    if ui.button("复制图像").clicked() {
                        let result = self
                            .images
                            .get(self.selected)
                            .ok_or_else(|| "没有可复制的图像".to_string())
                            .and_then(copy_image);
                        self.clipboard_status = match result {
                            Ok(()) => "已复制到剪贴板".to_string(),
                            Err(error) => format!("复制失败：{error}"),
                        };
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("放大").clicked() {
                        self.fit = false;
                        self.zoom = rscmd_gui::zoom_in(self.zoom);
                    }
                    if ui.button("缩小").clicked() {
                        self.fit = false;
                        self.zoom = rscmd_gui::zoom_out(self.zoom);
                    }
                    if ui.button("实际大小").clicked() {
                        self.fit = false;
                        self.zoom = 1.0;
                    }
                    if ui.button("适应窗口").clicked() {
                        self.fit = true;
                    }
                });
                ui.menu_button("帮助", |ui| {
                    if ui.button("关于").clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(&self.font_status);
                ui.separator();
                ui.label(&self.clipboard_status);
            });
        });

        egui::SidePanel::left("resources")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.heading("POC 图像");
                ui.label("正式阶段会替换为 PE 资源树。");
                ui.separator();
                for index in 0..self.images.len() {
                    let image = &self.images[index];
                    let label = format!("{}  ({}×{})", image.name, image.width, image.height);
                    if ui.selectable_label(self.selected == index, label).clicked() {
                        self.selected = index;
                        self.fit = true;
                    }
                }
            });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(image) = self.images.get(self.selected) {
                    ui.label(format!(
                        "{}：{}×{}，倍率 {:.0}%",
                        image.name,
                        image.width,
                        image.height,
                        self.zoom * 100.0
                    ));
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(selected) = self.images.get(self.selected) else {
                ui.centered_and_justified(|ui| ui.label("没有可显示的图像"));
                return;
            };
            let (width, height) = (selected.width, selected.height);
            if self.fit {
                self.zoom = rscmd_gui::fit_zoom(
                    width,
                    height,
                    ui.available_width() - 16.0,
                    ui.available_height() - 16.0,
                );
            }
            let size = egui::vec2(width as f32 * self.zoom, height as f32 * self.zoom);
            let image = &mut self.images[self.selected];
            image.ensure_texture(ctx);
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(texture) = &image.texture {
                        ui.add(egui::Image::new(egui::load::SizedTexture::new(
                            texture.id(),
                            size,
                        )));
                    }
                });
        });

        egui::Window::new("关于")
            .open(&mut self.show_about)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label("reshackw POC");
                ui.label("验证 egui、中文字体、ICO/BMP 解码与剪贴板。");
            });
    }
}

fn install_cjk_font(ctx: &egui::Context) -> Option<String> {
    let path = rscmd_gui::find_cjk_font(&fonts_dir())?;
    let bytes = std::fs::read(&path).ok()?;
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_string(), egui::FontData::from_owned(bytes));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "cjk".to_string());
    }
    ctx.set_fonts(fonts);
    Some(path.display().to_string())
}

fn fonts_dir() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Windows"))
        .join("Fonts")
}

fn copy_image(image: &PocImage) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width as usize,
            height: image.height as usize,
            bytes: std::borrow::Cow::Borrowed(&image.rgba),
        })
        .map_err(|error| error.to_string())
}

fn sample_inputs(args: &[String]) -> Vec<(String, Vec<u8>)> {
    let mut inputs = vec![
        ("示例 BMP（32×32）".to_string(), bmp_fixture()),
        ("示例 ICO（16×16）".to_string(), ico_fixture()),
    ];
    inputs.extend(external_inputs(args));
    inputs
}

fn external_inputs(args: &[String]) -> Vec<(String, Vec<u8>)> {
    let mut inputs = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--open" && index + 1 < args.len() {
            let path = &args[index + 1];
            match std::fs::read(path) {
                Ok(bytes) => inputs.push((path.clone(), bytes)),
                Err(error) => eprintln!("读取 {path} 失败：{error}"),
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    inputs
}

fn bmp_fixture() -> Vec<u8> {
    let mut image = image::RgbaImage::new(32, 32);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let red = x < 16;
        let green = y < 16;
        *pixel = image::Rgba([
            if red { 220 } else { 40 },
            if green { 180 } else { 40 },
            60,
            255,
        ]);
    }
    let mut output = Vec::new();
    image::codecs::bmp::BmpEncoder::new(&mut output)
        .encode(image.as_raw(), 32, 32, image::ExtendedColorType::Rgba8)
        .expect("BMP 编码不应失败");
    output
}

fn ico_fixture() -> Vec<u8> {
    let mut image = image::RgbaImage::new(16, 16);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let border = x == 0 || y == 0 || x == 15 || y == 15;
        *pixel = if border {
            image::Rgba([255, 255, 255, 255])
        } else {
            image::Rgba([30, 90, 200, 255])
        };
    }
    let mut output = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::ico::IcoEncoder::new(&mut output),
        image.as_raw(),
        16,
        16,
        image::ExtendedColorType::Rgba8,
    )
    .expect("ICO 编码不应失败");
    output
}

fn self_test(args: &[String]) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();

    let font_path = rscmd_gui::find_cjk_font(&fonts_dir())
        .ok_or_else(|| format!("未在 {} 找到中文字体", fonts_dir().display()))?;
    let font_bytes = std::fs::read(&font_path).map_err(|error| format!("读取字体失败：{error}"))?;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_string(),
        egui::FontData::from_owned(font_bytes.clone()),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "cjk".to_string());
    }
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts);
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    lines.push(format!(
        "font: ok ({}, {} bytes)",
        font_path.display(),
        font_bytes.len()
    ));

    let bmp = PocImage::from_bytes("bmp", &bmp_fixture())?;
    let ico = PocImage::from_bytes("ico", &ico_fixture())?;
    if (bmp.width, bmp.height) != (32, 32) {
        return Err(format!("BMP 尺寸异常：{}×{}", bmp.width, bmp.height));
    }
    if (ico.width, ico.height) != (16, 16) {
        return Err(format!("ICO 尺寸异常：{}×{}", ico.width, ico.height));
    }
    lines.push(format!("image bmp: ok ({}×{})", bmp.width, bmp.height));
    lines.push(format!("image ico: ok ({}×{})", ico.width, ico.height));

    for (name, bytes) in external_inputs(args) {
        match PocImage::from_bytes(&name, &bytes) {
            Ok(image) => lines.push(format!(
                "image {name}: ok ({}×{})",
                image.width, image.height
            )),
            Err(error) => return Err(format!("{name} 解码失败：{error}")),
        }
    }

    let text = rscmd_gui::decode_text(&[0xFF, 0xFE, 0x2D, 0x4E, 0x87, 0x65]);
    if text != "中文" {
        return Err(format!("文本解码异常：{text}"));
    }
    lines.push("text utf16le: ok".to_string());

    match clipboard_roundtrip(&bmp) {
        Ok(()) => lines.push("clipboard: ok".to_string()),
        Err(error) => lines.push(format!("clipboard: skipped ({error})")),
    }

    Ok(lines)
}

fn clipboard_roundtrip(image: &PocImage) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width as usize,
            height: image.height as usize,
            bytes: std::borrow::Cow::Borrowed(&image.rgba),
        })
        .map_err(|error| error.to_string())?;
    let received = clipboard.get_image().map_err(|error| error.to_string())?;
    if received.width != image.width as usize || received.height != image.height as usize {
        return Err(format!(
            "尺寸不一致：{}×{}",
            received.width, received.height
        ));
    }
    Ok(())
}
