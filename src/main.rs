use std::{collections::HashMap, fs, io::Cursor, num::NonZeroU32};

use eframe::egui;
use egui::{
    Color32, FontData, FontDefinitions, FontFamily, Image, Pos2, Rect, Rounding, Sense, Stroke,
};
use image::{DynamicImage, GenericImage, GenericImageView, ImageFormat, ImageReader, Rgba};

fn main() -> eframe::Result {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Image Mosaic Editor",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(MyEguiApp::new(cc)))
        }),
    )
}

struct MyEguiApp {
    image: Option<TargetImage>,
    mosaic_center_distance_pixels: NonZeroU32,
}

struct TargetImage {
    raw_file_name: String,
    saving_image: DynamicImage,
    processing_image: DynamicImage,
    selected_area: Option<Area>,

    cached_bytes: Vec<u8>,
}

/// What is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Area {
    start_pos: egui::Vec2,
    end_pos: egui::Vec2,
}

impl MyEguiApp {
    fn new(context: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = FontDefinitions::default();

        // Install my own font (maybe supporting non-latin characters):
        fonts.font_data.insert(
            "my_font".to_owned(),
            std::sync::Arc::new(
                // .ttf and .otf supported
                FontData::from_static(include_bytes!("../NotoSansJP-Regular.ttf")),
            ),
        );

        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "my_font".to_owned());

        fonts
            .families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .push("my_font".to_owned());
        context.egui_ctx.set_fonts(fonts);

        Self {
            image: None,
            mosaic_center_distance_pixels: NonZeroU32::new(5).unwrap(),
        }
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if ui.button(" 画像を開く").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    let decoder = ImageReader::open(path.display().to_string().clone())
                        .unwrap()
                        .into_decoder()
                        .unwrap();

                    let _image = DynamicImage::from_decoder(decoder).unwrap();
                    let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                    let bytes = get_bytes(&_image, &file_name);

                    self.image = Some(TargetImage {
                        raw_file_name: file_name,
                        saving_image: _image.clone(),
                        processing_image: _image,
                        selected_area: None,
                        cached_bytes: bytes,
                    });
                }
            }

            ui.add(
                egui::Slider::new(
                    &mut self.mosaic_center_distance_pixels,
                    NonZeroU32::new(1).unwrap()..=NonZeroU32::new(20).unwrap(),
                )
                .text("モザイクの荒さ"),
            );

            ui.separator();

            if let Some(image) = &mut self.image {
                let uri = format!("bytes://{}", image.raw_file_name);
                let image_widget = Image::from_bytes(uri.clone(), image.cached_bytes.clone());
                let image_widget_response = ui.add(image_widget);

                let drag_res = image_widget_response.interact(Sense::drag());

                if drag_res.drag_started() {
                    image.selected_area = Some(Area {
                        start_pos: drag_res.interact_pointer_pos.unwrap()
                            - drag_res.interact_rect.left_top(),
                        end_pos: drag_res.interact_pointer_pos.unwrap()
                            - drag_res.interact_rect.left_top(),
                    });
                }
                if drag_res.dragged() {
                    if let Some(area) = &mut image.selected_area {
                        area.end_pos = drag_res.interact_pointer_pos.unwrap()
                            - drag_res.interact_rect.left_top();
                    }
                }

                if drag_res.drag_stopped() {
                    if let Some(area) = &mut image.selected_area {
                        // 画像の加工
                        let mut proccesing_image = image.saving_image.clone();
                        let radius: u32 = self.mosaic_center_distance_pixels.get();
                        let diameter = radius * 2 + 1;

                        let (width, height) = proccesing_image.dimensions();
                        let ratio = get_image_ratio(&proccesing_image, image_widget_response.rect);

                        let min_pos = area.start_pos.min(area.end_pos);
                        let max_pos = area.start_pos.max(area.end_pos);

                        let mut pixel_cache: HashMap<(u32, u32), Rgba<u8>> = HashMap::new();
                        for x in 0..width {
                            for y in 0..height {
                                let a = Pos2::new(x as f32, y as f32) - (ratio * min_pos);
                                let b = (-1. * Pos2::new(x as f32, y as f32)) + (ratio * max_pos);
                                if a.x > 0. && a.y > 0. && b.x > 0. && b.y > 0. {
                                    if x % diameter != radius || y % diameter != radius {
                                        let center_x = x - (x % diameter) + radius;
                                        let center_y = y - (y % diameter) + radius;
                                        if center_x < width && center_y < height {
                                            let pixel = match pixel_cache.get(&(center_x, center_y))
                                            {
                                                Some(a) => a.clone(),
                                                None => {
                                                    let _pixel = proccesing_image
                                                        .get_pixel(center_x, center_y);
                                                    pixel_cache
                                                        .insert((center_x, center_y), _pixel);
                                                    _pixel
                                                }
                                            };

                                            proccesing_image.put_pixel(x, y, pixel);
                                        }
                                    }
                                }
                            }
                        }
                        ctx.forget_image(&uri);
                        image.cached_bytes = get_bytes(&proccesing_image, &image.raw_file_name);
                        image.processing_image = proccesing_image;
                    }
                }

                if let Some(area) = &mut image.selected_area {
                    ui.painter().rect(
                        Rect::from_two_pos(
                            drag_res.rect.left_top() + area.start_pos,
                            drag_res.rect.left_top() + area.end_pos,
                        ),
                        Rounding::ZERO,
                        Color32::TRANSPARENT,
                        Stroke::new(1., Color32::RED),
                    );
                }

                ui.horizontal(|ui| {
                    ui.add_enabled_ui(image.selected_area.is_some(), |ui| {
                        if ui.button("モザイク確定").clicked() {
                            image.saving_image = image.processing_image.clone();
                            image.selected_area = None;
                        }
                    });

                    if ui.button("保存").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            let mut saving_file_path = path.join(&image.raw_file_name);
                            if fs::exists(saving_file_path.clone()).unwrap() {
                                saving_file_path =
                                    path.join(format!("mosaic_{}", &image.raw_file_name));
                            }
                            image.saving_image.save(saving_file_path).unwrap();
                        }
                    }
                });

                // add horizontal border
                ui.separator();
                let ratio = get_image_ratio(&image.processing_image, image_widget_response.rect);

                ui.horizontal(|ui| {
                    ui.label(&format!(
                        "画像サイズ: {} x {}",
                        image.processing_image.width(),
                        image.processing_image.height()
                    ));

                    ui.label(&format!("表示比率: {}", ratio));
                });
            }
        });
    }
}

fn get_bytes(dynamic_image: &DynamicImage, file_name: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    dynamic_image
        .write_to(
            &mut Cursor::new(&mut bytes),
            ImageFormat::from_path(file_name).unwrap(),
        )
        .unwrap();
    bytes
}

fn get_image_ratio(image: &DynamicImage, rect: Rect) -> f32 {
    let (width, _) = image.dimensions();
    let real_image_width = rect.width();
    width as f32 / real_image_width
}
