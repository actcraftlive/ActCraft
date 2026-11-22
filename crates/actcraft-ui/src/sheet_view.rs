//! The Sheet music tab: a song's PDF (sheet music, lyrics, notes), page under page. Pages render
//! on a worker thread through craft-pdf, so the window never waits for them; only pages in view
//! are rendered, at the width they are shown.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};

use actcraft_engine::Engine;
use actcraft_show::Song;
use egui::{Color32, ColorImage, RichText, Ui, vec2};
use serde_json::json;

use crate::flows::{FileAsk, FileKind, FileRequest};
use crate::{ActCraftApp, tr};

/// The PDF file type the picker offers.
pub const PDF_EXTENSIONS: &[&str] = &["pdf"];
/// Most pages read from a PDF.
const MAX_PAGES: usize = 500;
/// Rendered widths are rounded up to a multiple of this, so resizing the window by a few pixels
/// doesn't render every page again.
const WIDTH_STEP: u32 = 128;
/// Widest page render in pixels.
const MAX_WIDTH: u32 = 3072;
/// Rendered pages kept as textures before the oldest are dropped.
const MAX_TEXTURES: usize = 24;

/// A page render: page index, width in pixels, colours inverted.
type PageKey = (usize, u32, bool);
type Done = Arc<Mutex<Vec<(PageKey, Result<ColorImage, String>)>>>;

/// What the Sheet music tab shows when a song has both a PDF and a score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SheetSource {
    #[default]
    Notation,
    Pdf,
}

/// How the Sheet music tab shows pages.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SheetLook {
    /// Page width as a share of the tab's width.
    pub zoom: f32,
    /// White on black, for a dark stage.
    pub invert: bool,
    /// Scroll through the pages at a constant speed while the song plays.
    pub follow: bool,
}

impl Default for SheetLook {
    fn default() -> Self {
        Self { zoom: 1.0, invert: false, follow: false }
    }
}

/// An open PDF and its rendered pages.
pub(crate) struct SheetDoc {
    pub path: String,
    /// Each page's size in points.
    pub pages: Vec<(f32, f32)>,
    jobs: mpsc::Sender<PageKey>,
    done: Done,
    textures: HashMap<PageKey, egui::TextureHandle>,
    /// Least recently shown first.
    order: Vec<PageKey>,
    pending: HashSet<PageKey>,
    failed: HashSet<PageKey>,
}

impl SheetDoc {
    /// Reads the page sizes now and starts the worker that renders pages.
    pub fn open(path: &str, bytes: Vec<u8>, ctx: egui::Context) -> Result<Self, String> {
        let pdf = craft_pdf::open_slice(&bytes, None).map_err(|e| e.to_string())?;
        let pages: Vec<(f32, f32)> = pdf.pages().iter().take(MAX_PAGES).map(|p| p.render_dimensions()).collect();
        drop(pdf);
        if pages.is_empty() || pages.iter().any(|&(w, h)| !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0)) {
            return Err(tr("The PDF has no pages ActCraft can show.").to_owned());
        }
        let (jobs, rx) = mpsc::channel::<PageKey>();
        let done: Done = Arc::new(Mutex::new(Vec::new()));
        let out = Arc::clone(&done);
        std::thread::Builder::new()
            .name("actcraft-pdf".into())
            .spawn(move || {
                let Ok(pdf) = craft_pdf::open_slice(&bytes, None) else { return };
                let cache = craft_pdf::raster::RenderCache::new();
                let settings = craft_pdf::interpret::InterpreterSettings::default();
                // Ends when the document is closed (the sender is dropped).
                while let Ok(key) = rx.recv() {
                    let result = render_page(&pdf, &cache, &settings, key);
                    if let Ok(mut d) = out.lock() {
                        d.push((key, result));
                    }
                    ctx.request_repaint();
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            path: path.to_owned(),
            pages,
            jobs,
            done,
            textures: HashMap::new(),
            order: Vec::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
        })
    }

    /// Takes finished renders as textures.
    fn collect(&mut self, ctx: &egui::Context) {
        let finished: Vec<(PageKey, Result<ColorImage, String>)> =
            self.done.lock().map(|mut d| d.drain(..).collect()).unwrap_or_default();
        for (key, result) in finished {
            self.pending.remove(&key);
            match result {
                Ok(image) => {
                    let name = format!("pdf-page-{}-{}-{}", key.0, key.1, key.2);
                    self.textures.insert(key, ctx.load_texture(name, image, egui::TextureOptions::LINEAR));
                    self.order.push(key);
                }
                Err(_) => {
                    self.failed.insert(key);
                }
            }
        }
        while self.order.len() > MAX_TEXTURES {
            let old = self.order.remove(0);
            self.textures.remove(&old);
        }
    }

    /// The texture for a page, asking the worker for it when it isn't there yet.
    fn texture(&mut self, key: PageKey) -> Option<&egui::TextureHandle> {
        if self.textures.contains_key(&key) {
            self.order.retain(|k| *k != key);
            self.order.push(key);
            return self.textures.get(&key);
        }
        if !self.pending.contains(&key) && !self.failed.contains(&key) && self.jobs.send(key).is_ok() {
            self.pending.insert(key);
        }
        None
    }
}

/// Renders one page at `width` pixels, white paper (or black, inverted).
fn render_page<'a>(
    pdf: &'a craft_pdf::syntax::Pdf,
    cache: &craft_pdf::raster::RenderCache<'a>,
    settings: &craft_pdf::interpret::InterpreterSettings,
    (index, width, invert): PageKey,
) -> Result<ColorImage, String> {
    let pages = pdf.pages();
    let page = pages.get(index).ok_or("no such page")?;
    let (w, _) = page.render_dimensions();
    let scale = width as f32 / w.max(1.0);
    let options =
        craft_pdf::raster::Options { scale: [scale, scale], background: [255, 255, 255, 255], ..Default::default() };
    let raster = craft_pdf::raster::render(page, cache, settings, &options).map_err(|e| format!("{e:?}"))?;
    let size = [raster.width as usize, raster.height as usize];
    let mut bytes = raster.bytes().to_vec();
    if invert {
        invert_colours(&mut bytes);
    }
    Ok(ColorImage::from_rgba_premultiplied(size, &bytes))
}

/// White on black: each colour channel inverted, alpha kept (the paper is opaque).
fn invert_colours(rgba: &mut [u8]) {
    for [r, g, b, _] in rgba.as_chunks_mut::<4>().0 {
        *r = 255 - *r;
        *g = 255 - *g;
        *b = 255 - *b;
    }
}

/// The render width for a page shown `shown` points wide on a screen with `ppp` pixels per point.
fn render_width(shown: f32, ppp: f32) -> u32 {
    let px = (shown * ppp).ceil().max(1.0) as u32;
    px.div_ceil(WIDTH_STEP).saturating_mul(WIDTH_STEP).clamp(WIDTH_STEP, MAX_WIDTH)
}

impl ActCraftApp {
    pub(crate) fn sheet_tab(&mut self, ui: &mut Ui, e: &mut Engine, song: &Song) {
        let t = self.tokens;
        // The open document follows the song's PDF.
        let wanted = song.pdf_file.clone();
        if self.sheet.as_ref().map(|s| s.path.as_str()) != wanted.as_deref() {
            self.sheet = None;
            if let Some(path) = &wanted {
                match e.services().read_file(path).and_then(|b| SheetDoc::open(path, b, ui.ctx().clone())) {
                    Ok(doc) => self.sheet = Some(doc),
                    Err(err) => {
                        ui.label(
                            RichText::new(format!("{} {err}", tr("The PDF could not be opened:"))).color(t.danger),
                        );
                    }
                }
            }
        }
        let score = song.notation_file.is_some();
        // With both a PDF and a score, the person picks which one shows.
        let source = match (wanted.is_some(), score) {
            (true, false) => SheetSource::Pdf,
            (false, true) => SheetSource::Notation,
            _ => self.sheet_source,
        };
        ui.horizontal_wrapped(|ui| {
            if wanted.is_some() && score {
                ui.selectable_value(&mut self.sheet_source, SheetSource::Notation, tr("Notation"));
                ui.selectable_value(&mut self.sheet_source, SheetSource::Pdf, tr("PDF"));
                ui.separator();
            }
            let attach = if wanted.is_some() { tr("Choose another PDF\u{2026}") } else { tr("Attach PDF\u{2026}") };
            if ui.button(attach).on_hover_text(tr("Sheet music, lyrics or notes as a PDF file")).clicked() {
                self.ask_file(FileRequest {
                    ask: FileAsk::AttachPdf { song: song.id },
                    kind: FileKind::Open,
                    title: tr("Attach PDF"),
                    filters: vec![(tr("PDF file"), PDF_EXTENSIONS)],
                    file_name: None,
                    directory: None,
                });
            }
            if wanted.is_some() && ui.button(tr("Remove PDF")).clicked() {
                self.run(e, "song.set", json!({"song": song.id, "pdf_file": null}));
            }
            let attach =
                if score { tr("Choose another score\u{2026}") } else { tr("Attach notation\u{2026}") };
            if ui
                .button(attach)
                .on_hover_text(tr("Notes and tablature as a MusicXML file (.musicxml, .xml or .mxl); notation and tab editors can export it"))
                .clicked()
            {
                self.ask_file(FileRequest {
                    ask: FileAsk::AttachNotation { song: song.id },
                    kind: FileKind::Open,
                    title: tr("Attach notation"),
                    filters: vec![(tr("MusicXML score"), crate::notation_view::NOTATION_EXTENSIONS)],
                    file_name: None,
                    directory: None,
                });
            }
            if score && ui.button(tr("Remove notation")).clicked() {
                self.run(e, "song.set", json!({"song": song.id, "notation_file": null}));
            }
        });
        if score && source == SheetSource::Notation {
            ui.separator();
            self.notation_view(ui, e, song);
            return;
        }
        if let Some(doc) = &self.sheet {
            ui.horizontal_wrapped(|ui| {
                let n = doc.pages.len().to_string();
                ui.label(actcraft_engine::fill(tr("{count} pages"), &[("count", &n)]));
                ui.spacing_mut().slider_width = 120.0;
                ui.add(egui::Slider::new(&mut self.sheet_view.zoom, 0.4..=2.0).show_value(false).text(tr("Zoom")));
                ui.checkbox(&mut self.sheet_view.invert, tr("Invert colours"))
                    .on_hover_text(tr("White on black, for a dark stage"));
                ui.checkbox(&mut self.sheet_view.follow, tr("Follow playback"))
                    .on_hover_text(tr("Scroll through the pages at a constant speed while the song plays"));
            });
        }
        ui.separator();
        let Some(doc) = self.sheet.as_mut() else {
            if wanted.is_none() {
                ui.add_space(20.0);
                ui.label(RichText::new(tr("No sheet music yet. Attach a PDF (sheet music, a lyrics sheet or notes) or a score as MusicXML (notes and tablature) to see it here while you play.")).color(t.text_dim));
            }
            return;
        };
        doc.collect(ui.ctx());
        let look = self.sheet_view;
        let available = ui.available_width();
        let shown_w = (available * look.zoom).max(100.0);
        let px = render_width(shown_w, ui.ctx().pixels_per_point());
        let gap = 12.0;
        let heights: Vec<f32> = doc.pages.iter().map(|&(w, h)| shown_w * h / w).collect();
        let content: f32 = heights.iter().sum::<f32>() + gap * heights.len() as f32;
        let mut area = egui::ScrollArea::both().id_salt(("sheet", song.id)).auto_shrink([false, false]);
        if look.follow && e.status.playing {
            let span = (song.start, song.end.unwrap_or(song.duration));
            if span.1 > span.0 {
                let frac = ((e.status.time - span.0) / (span.1 - span.0)).clamp(0.0, 1.0) as f32;
                let view = ui.available_height();
                area = area.vertical_scroll_offset(frac * (content - view).max(0.0));
                ui.ctx().request_repaint();
            }
        }
        let paper = if look.invert { Color32::BLACK } else { Color32::WHITE };
        area.show_viewport(ui, |ui, viewport| {
            ui.set_min_size(vec2(shown_w, content));
            let origin = ui.min_rect().min;
            let mut y = 0.0;
            for (i, h) in heights.iter().copied().enumerate() {
                let rect = egui::Rect::from_min_size(origin + vec2(0.0, y), vec2(shown_w, h));
                let local = egui::Rect::from_min_size(egui::pos2(0.0, y), vec2(shown_w, h));
                if local.intersects(viewport.expand(h * 0.5)) {
                    match doc.texture((i, px, look.invert)) {
                        Some(tex) => {
                            ui.painter().image(
                                tex.id(),
                                rect,
                                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }
                        None => {
                            ui.painter().rect_filled(rect, 2.0, paper);
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                actcraft_engine::fill(tr("Page {page}"), &[("page", &(i + 1).to_string())]),
                                egui::FontId::proportional(16.0),
                                Color32::GRAY,
                            );
                        }
                    }
                }
                y += h + gap;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_widths_step_and_stay_bounded() {
        assert_eq!(render_width(500.0, 1.0), 512);
        assert_eq!(render_width(500.0, 2.0), 1024);
        assert_eq!(render_width(1.0, 1.0), WIDTH_STEP);
        assert_eq!(render_width(10_000.0, 2.0), MAX_WIDTH);
        let mut px = [0, 128, 255, 255];
        invert_colours(&mut px);
        assert_eq!(px, [255, 127, 0, 255]);
    }

    /// A one-page PDF, 200 x 100 points, with a black square in its lower left quarter.
    fn minimal_pdf() -> Vec<u8> {
        let content = "0 0 0 rg 0 0 100 50 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ];
        let mut out = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
        }
        let xref = out.len();
        out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
        for o in offsets {
            out.push_str(&format!("{o:010} 00000 n \n"));
        }
        out.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
        out.into_bytes()
    }

    #[test]
    fn a_pdf_opens_and_renders_its_page_plain_and_inverted() {
        let bytes = minimal_pdf();
        let doc = SheetDoc::open("a.pdf", bytes.clone(), egui::Context::default());
        assert_eq!(doc.as_ref().map(|d| d.pages.clone()).ok(), Some(vec![(200.0, 100.0)]));
        let pdf = craft_pdf::open_slice(&bytes, None);
        assert!(pdf.is_ok());
        let Ok(pdf) = pdf else { return };
        let cache = craft_pdf::raster::RenderCache::new();
        let settings = craft_pdf::interpret::InterpreterSettings::default();
        let pixel = |img: &ColorImage, x: usize, y: usize| img.pixels.get(y * img.size[0] + x).copied();
        let plain = render_page(&pdf, &cache, &settings, (0, 256, false));
        assert!(plain.is_ok(), "{:?}", plain.as_ref().err());
        let Ok(plain) = plain else { return };
        assert_eq!(plain.size, [256, 128]);
        // The square is in the lower left (PDF y grows upwards); the rest is paper.
        assert_eq!(pixel(&plain, 10, 120), Some(Color32::BLACK));
        assert_eq!(pixel(&plain, 250, 5), Some(Color32::WHITE));
        let inverted = render_page(&pdf, &cache, &settings, (0, 256, true)).ok();
        assert_eq!(inverted.as_ref().and_then(|i| pixel(i, 10, 120)), Some(Color32::WHITE));
        assert_eq!(inverted.as_ref().and_then(|i| pixel(i, 250, 5)), Some(Color32::BLACK));
        assert!(render_page(&pdf, &cache, &settings, (3, 256, false)).is_err(), "no such page");
        assert!(SheetDoc::open("b.pdf", b"not a pdf".to_vec(), egui::Context::default()).is_err());
    }
}
