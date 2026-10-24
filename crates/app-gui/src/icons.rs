//! Icons from the open-licensed Lucide set (ISC licence,
//! `assets/icons/LICENSE-Lucide.txt`), bundled as their original SVG files
//! and rasterized with resvg at the size and screen scale they are shown at.
//! Each is drawn in one colour (Lucide icons are single-colour line art),
//! always beside a text label, so an icon never carries meaning alone.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, Color32, Rect, TextureHandle, TextureOptions};

/// A bundled Lucide icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Table,
    FolderOpen,
    Download,
    FileText,
    Sheet,
    Database,
    Globe,
    ClipboardPaste,
    Folder,
    ChartColumn,
    ChartCandlestick,
    ChartScatter,
    Activity,
    FlaskConical,
    ChartLine,
    BookOpen,
    Lightbulb,
}

impl Icon {
    /// The icon's SVG source, as published by Lucide.
    fn svg(self) -> &'static str {
        match self {
            Self::Table => include_str!("../assets/icons/table.svg"),
            Self::FolderOpen => include_str!("../assets/icons/folder-open.svg"),
            Self::Download => include_str!("../assets/icons/download.svg"),
            Self::FileText => include_str!("../assets/icons/file-text.svg"),
            Self::Sheet => include_str!("../assets/icons/sheet.svg"),
            Self::Database => include_str!("../assets/icons/database.svg"),
            Self::Globe => include_str!("../assets/icons/globe.svg"),
            Self::ClipboardPaste => include_str!("../assets/icons/clipboard-paste.svg"),
            Self::Folder => include_str!("../assets/icons/folder.svg"),
            Self::ChartColumn => include_str!("../assets/icons/chart-column.svg"),
            Self::ChartCandlestick => include_str!("../assets/icons/chart-candlestick.svg"),
            Self::ChartScatter => include_str!("../assets/icons/chart-scatter.svg"),
            Self::Activity => include_str!("../assets/icons/activity.svg"),
            Self::FlaskConical => include_str!("../assets/icons/flask-conical.svg"),
            Self::ChartLine => include_str!("../assets/icons/chart-line.svg"),
            Self::BookOpen => include_str!("../assets/icons/book-open.svg"),
            Self::Lightbulb => include_str!("../assets/icons/lightbulb.svg"),
        }
    }
}

/// Rasterize `icon` as a white image `px` pixels square (the colour is
/// applied as a tint when painting), or `None` if the SVG cannot be read.
fn rasterize(icon: Icon, px: u32) -> Option<egui::ColorImage> {
    // Lucide strokes in `currentColor`; white tints to any colour.
    let svg = icon.svg().replace("currentColor", "#ffffff");
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
    let scale = px as f32 / tree.size().width();
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Some(egui::ColorImage::from_rgba_premultiplied([px as usize, px as usize], pixmap.data()))
}

/// Textures already made, by icon and pixel size.
type Cache = Arc<egui::mutex::Mutex<HashMap<(Icon, u32), Option<TextureHandle>>>>;

/// The texture of `icon` for a square of side `size` points on this screen.
fn texture(ctx: &egui::Context, icon: Icon, size: f32) -> Option<TextureHandle> {
    let px = (size * ctx.pixels_per_point()).round().max(1.0) as u32;
    let cache: Cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::new("lucide_icons")).clone());
    cache
        .lock()
        .entry((icon, px))
        .or_insert_with(|| {
            rasterize(icon, px)
                .map(|image| ctx.load_texture(format!("lucide-{icon:?}-{px}"), image, TextureOptions::LINEAR))
        })
        .clone()
}

/// `icon` as an image of side `size` points in `color`, for buttons that
/// show an image beside their text.
pub fn image(ctx: &egui::Context, icon: Icon, size: f32, color: Color32) -> Option<egui::Image<'static>> {
    let texture = texture(ctx, icon, size)?;
    Some(egui::Image::new(egui::load::SizedTexture::new(texture.id(), egui::vec2(size, size))).tint(color))
}

/// Paint `icon` filling the square `rect`, in `color`.
pub fn paint(ui: &egui::Ui, icon: Icon, rect: Rect, color: Color32) {
    if let Some(texture) = texture(ui.ctx(), icon, rect.width().max(rect.height())) {
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter().image(texture.id(), rect, uv, color);
    }
}

/// Allocate a square of side `size` points and paint `icon` in it, in `color`.
pub fn show(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui, icon, rect, color);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bundled icon reads and draws something.
    #[test]
    fn every_icon_rasterizes() {
        for icon in [
            Icon::Table,
            Icon::FolderOpen,
            Icon::Download,
            Icon::FileText,
            Icon::Sheet,
            Icon::Database,
            Icon::Globe,
            Icon::ClipboardPaste,
            Icon::Folder,
            Icon::ChartColumn,
            Icon::ChartCandlestick,
            Icon::ChartScatter,
            Icon::Activity,
            Icon::FlaskConical,
            Icon::ChartLine,
            Icon::BookOpen,
            Icon::Lightbulb,
        ] {
            let image = rasterize(icon, 36).unwrap_or_else(|| panic!("{icon:?} did not rasterize"));
            assert_eq!(image.size, [36, 36]);
            let inked = image.pixels.iter().filter(|p| p.a() > 0).count();
            assert!(inked > 20, "{icon:?} drew {inked} pixels");
        }
    }
}
