//! The Sheet music tab: a song's PDF (sheet music, lyrics, notes), page under page. Pages render
//! on a worker thread through craft-pdf, so the window never waits for them; only pages in view
//! are rendered, at the width they are shown.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};

use actcraft_engine::Engine;
use actcraft_show::{PdfMark, PdfMarkKind, Song};
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

/// What a pointer on a page does in the Sheet music tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MarkTool {
    /// Pages are only looked at (and scrolled by dragging).
    #[default]
    View,
    Pen,
    Highlight,
    Note,
}

/// Colours a mark can have, by name, so the choice never rests on colour alone.
pub(crate) const MARK_COLOURS: [(&str, &str); 4] =
    [("Red", "#e5484d"), ("Blue", "#4f9cf0"), ("Green", "#2f9e5b"), ("Yellow", "#f2b43c")];

/// Marking on a PDF: the tool, the colour (index into [`MARK_COLOURS`]) and the stroke or box
/// being drawn.
#[derive(Debug, Clone, Default)]
pub(crate) struct MarkEdit {
    pub tool: MarkTool,
    pub colour: usize,
    /// The page and points of the mark being drawn.
    pub draft: Option<(u32, Vec<[f32; 2]>)>,
}

impl MarkEdit {
    fn colour_hex(&self) -> &'static str {
        MARK_COLOURS.get(self.colour).map_or("#e5484d", |c| c.1)
    }
}

/// A finished stroke or box, or a spot where a note goes, as `pdf.mark` params.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MarkRequest {
    Mark(serde_json::Value),
    NoteAt { page: u32, at: [f32; 2] },
}

/// The point `p` as a fraction of `rect`, from its top left.
fn fraction(rect: egui::Rect, p: egui::Pos2) -> [f32; 2] {
    let f = (p - rect.min) / rect.size().max(vec2(1.0, 1.0));
    [f.x.clamp(0.0, 1.0), f.y.clamp(0.0, 1.0)]
}

/// The pointer's next step on a page while `edit` marks: a stroke grows while dragged, a box
/// spans the drag, a click asks for a note. Returns what is finished.
fn mark_input(edit: &mut MarkEdit, page: u32, rect: egui::Rect, r: &egui::Response) -> Option<MarkRequest> {
    let at = r.interact_pointer_pos().map(|p| fraction(rect, p));
    match edit.tool {
        MarkTool::View => None,
        MarkTool::Note => {
            if r.clicked() {
                at.map(|at| MarkRequest::NoteAt { page, at })
            } else {
                None
            }
        }
        MarkTool::Pen | MarkTool::Highlight => {
            if r.drag_started()
                && let Some(at) = at
            {
                edit.draft = Some((page, vec![at]));
            }
            if r.dragged()
                && let (Some(at), Some((p, points))) = (at, edit.draft.as_mut())
                && *p == page
            {
                match edit.tool {
                    MarkTool::Highlight => {
                        points.truncate(1);
                        points.push(at);
                    }
                    _ => {
                        let moved = points.last().is_none_or(|l| (l[0] - at[0]).abs() + (l[1] - at[1]).abs() > 0.002);
                        if moved && points.len() < actcraft_show::limits::MAX_MARK_POINTS {
                            points.push(at);
                        }
                    }
                }
            }
            if r.drag_stopped()
                && let Some((p, points)) = edit.draft.take()
                && p == page
                && points.len() >= 2
            {
                let kind = if edit.tool == MarkTool::Highlight { "highlight" } else { "pen" };
                let points: Vec<[f32; 2]> =
                    if edit.tool == MarkTool::Highlight { points.into_iter().take(2).collect() } else { points };
                return Some(MarkRequest::Mark(json!({
                    "page": page, "kind": kind, "points": points, "colour": edit.colour_hex(),
                })));
            }
            None
        }
    }
}

/// Paints a page's marks over it; `rect` is the page as shown.
fn paint_marks<'a>(
    painter: &egui::Painter,
    rect: egui::Rect,
    marks: impl Iterator<Item = &'a PdfMark>,
    paper: Color32,
) {
    let at = |p: &[f32; 2]| rect.min + vec2(p[0] * rect.width(), p[1] * rect.height());
    let thick = (rect.width() * 0.003).clamp(1.5, 6.0);
    for m in marks {
        let [r, g, b] = actcraft_show::parse_colour(&m.colour).unwrap_or([229, 72, 77]);
        let colour = Color32::from_rgb(r, g, b);
        match m.kind {
            PdfMarkKind::Pen => {
                let pts: Vec<egui::Pos2> = m.points.iter().map(at).collect();
                painter.add(egui::Shape::line(pts, egui::Stroke::new(thick, colour)));
            }
            PdfMarkKind::Highlight => {
                if let (Some(p), Some(q)) = (m.points.first(), m.points.get(1)) {
                    let area = egui::Rect::from_two_pos(at(p), at(q));
                    painter.rect_filled(area, 2.0, Color32::from_rgba_unmultiplied(r, g, b, 80));
                }
            }
            PdfMarkKind::Note => {
                if let Some(p) = m.points.first() {
                    let size = (rect.width() * 0.022).clamp(11.0, 30.0);
                    let galley = painter.layout_no_wrap(m.text.clone(), egui::FontId::proportional(size), colour);
                    let bg = egui::Rect::from_min_size(at(p), galley.size()).expand(3.0);
                    painter.rect_filled(bg, 3.0, paper);
                    painter.rect_stroke(bg, 3.0, egui::Stroke::new(1.5, colour), egui::StrokeKind::Inside);
                    painter.galley(at(p), galley, colour);
                }
            }
        }
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

/// A note being written for a spot on a PDF: song, sheet (`None`: the song's own), page, spot,
/// words.
pub(crate) type NotePrompt = (u64, Option<usize>, u32, [f32; 2], String);

impl ActCraftApp {
    pub(crate) fn sheet_tab(&mut self, ui: &mut Ui, e: &mut Engine, song: &Song) {
        let t = self.tokens;
        self.sheet_picker(ui, e, song);
        // From here on the song as the picked sheet shows it.
        let pick = self.picked_sheet(song);
        let song = &song.with_sheet(pick);
        // The open document follows the song's PDF.
        let wanted = song.pdf_file.clone();
        if let Err(err) = load_pdf_into(&mut self.sheet, ui.ctx(), e, wanted.as_deref()) {
            ui.label(RichText::new(format!("{} {err}", tr("The PDF could not be opened:"))).color(t.danger));
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
                    ask: FileAsk::AttachPdf { song: song.id, sheet: pick },
                    kind: FileKind::Open,
                    title: tr("Attach PDF"),
                    filters: vec![(tr("PDF file"), PDF_EXTENSIONS)],
                    file_name: None,
                    directory: None,
                });
            }
            if wanted.is_some() && ui.button(tr("Remove PDF")).clicked() {
                self.run(e, "sheet.set", json!({"song": song.id, "sheet": pick, "pdf_file": null}));
            }
            let attach =
                if score { tr("Choose another score\u{2026}") } else { tr("Attach notation\u{2026}") };
            if ui
                .button(attach)
                .on_hover_text(tr("Notes and tablature as a MusicXML file (.musicxml, .xml or .mxl), which notation and tab editors export, or notes written out from a MIDI file (.mid)"))
                .clicked()
            {
                self.ask_file(FileRequest {
                    ask: FileAsk::AttachNotation { song: song.id, sheet: pick },
                    kind: FileKind::Open,
                    title: tr("Attach notation"),
                    filters: vec![(tr("MusicXML score or MIDI file"), crate::notation_view::NOTATION_EXTENSIONS)],
                    file_name: None,
                    directory: None,
                });
            }
            // The song's own MIDI file, written out as notes.
            if let Some(midi) = song.midi_file.clone().filter(|m| song.notation_file.as_deref() != Some(m.as_str()))
                && ui
                    .button(tr("Notes from the MIDI file"))
                    .on_hover_text(tr("Write the song's MIDI file out as notes; each MIDI track becomes a part"))
                    .clicked()
            {
                self.run(e, "notation.attach", json!({"song": song.id, "sheet": pick, "path": midi}));
                self.sheet_source = SheetSource::Notation;
            }
            if score && ui.button(tr("Remove notation")).clicked() {
                self.run(e, "sheet.set", json!({"song": song.id, "sheet": pick, "notation_file": null}));
            }
            if wanted.is_some() || score {
                ui.separator();
                self.stage_button(ui, crate::lyrics_view::StageShows::Sheet);
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
            self.mark_bar(ui, e, song);
        }
        ui.separator();
        let Some(doc) = self.sheet.as_mut() else {
            if wanted.is_none() {
                ui.add_space(20.0);
                ui.label(RichText::new(tr("No sheet music yet. Attach a PDF (sheet music, a lyrics sheet or notes) or a score as MusicXML (notes and tablature) to see it here while you play.")).color(t.text_dim));
            }
            return;
        };
        let status = (e.status.time, e.status.playing);
        let out = pdf_pages(ui, doc, self.sheet_view, song, status, "sheet", Some(&mut self.mark_edit));
        self.sheet_top_page = out.top_page;
        match out.request {
            Some(MarkRequest::Mark(mut prm)) => {
                prm["song"] = json!(song.id);
                prm["sheet"] = json!(pick);
                self.run(e, "pdf.mark", prm);
            }
            Some(MarkRequest::NoteAt { page, at }) => self.note_prompt = Some((song.id, pick, page, at, String::new())),
            None => {}
        }
        self.note_window(ui.ctx(), e);
    }

    /// The further sheet of `song` picked in the Sheet music tab; `None` for the song's own.
    pub(crate) fn picked_sheet(&self, song: &Song) -> Option<usize> {
        self.sheet_pick.filter(|&(id, i)| id == song.id && i < song.sheets.len()).map(|(_, i)| i)
    }

    /// The sheet rows: which sheet shows (the song's own or one for a musician), adding and
    /// removing sheets, the picked sheet's name and musician, and the band's musicians.
    fn sheet_picker(&mut self, ui: &mut Ui, e: &mut Engine, song: &Song) {
        let pick = self.picked_sheet(song);
        let name_of = |i: Option<usize>| match i.and_then(|i| song.sheets.get(i)) {
            None => tr("Main sheet").to_owned(),
            Some(s) if s.musician.is_empty() => s.name.clone(),
            Some(s) => format!("{} ({})", s.name, s.musician),
        };
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("Sheet"));
            let mut chosen = pick;
            egui::ComboBox::from_id_salt("sheet-pick")
                .selected_text(name_of(pick))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut chosen, None, name_of(None));
                    for i in 0..song.sheets.len() {
                        ui.selectable_value(&mut chosen, Some(i), name_of(Some(i)));
                    }
                })
                .response
                .labelled_by(label.id)
                .on_hover_text(tr("Each musician can have a sheet of their own for this song"));
            if chosen != pick {
                self.sheet_pick = chosen.map(|i| (song.id, i));
            }
            if let Some(i) = pick
                && ui.button(tr("Remove sheet")).clicked()
            {
                self.run(e, "sheet.remove", json!({"song": song.id, "sheet": i}));
                self.sheet_pick = None;
            }
            ui.separator();
            let label = ui.label(tr("New sheet"));
            ui.add(
                egui::TextEdit::singleline(&mut self.sheet_draft.0)
                    .desired_width(120.0)
                    .hint_text(tr("Name, such as Bass")),
            )
            .labelled_by(label.id);
            let ok = !self.sheet_draft.0.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new(tr("Add sheet"))).clicked() {
                let name = std::mem::take(&mut self.sheet_draft.0);
                if let Ok(v) = e.run("sheet.add", &json!({"song": song.id, "name": name})) {
                    self.sheet_pick = v["sheet"].as_u64().and_then(|i| usize::try_from(i).ok()).map(|i| (song.id, i));
                }
            }
        });
        if let Some((i, sheet)) = pick.and_then(|i| song.sheets.get(i).map(|s| (i, s))) {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Sheet name"));
                let id = egui::Id::unique(("sheet-name", song.id, i));
                if let Some(v) = crate::edit_text(ui, tr("Sheet name"), id, &sheet.name, 140.0)
                    && !v.trim().is_empty()
                {
                    self.run(e, "sheet.set", json!({"song": song.id, "sheet": i, "name": v}));
                }
                let label = ui.label(tr("For"));
                let mut musician = sheet.musician.clone();
                let anyone = tr("Anyone");
                egui::ComboBox::from_id_salt("sheet-musician")
                    .selected_text(if musician.is_empty() { anyone } else { musician.as_str() }.to_owned())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut musician, String::new(), anyone);
                        for m in &e.show.musicians {
                            ui.selectable_value(&mut musician, m.clone(), m);
                        }
                    })
                    .response
                    .labelled_by(label.id);
                if musician != sheet.musician {
                    self.run(e, "sheet.set", json!({"song": song.id, "sheet": i, "musician": musician}));
                }
            });
        }
        egui::CollapsingHeader::new(format!("{} ({})", tr("Musicians"), e.show.musicians.len()))
            .id_salt("sheet-musicians")
            .show(ui, |ui| {
                crate::help_view::help_button(ui, crate::help_view::Topic::SheetsPerMusician);
                let mut remove = None;
                for (i, m) in e.show.musicians.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(m);
                        let name = actcraft_engine::fill(tr("Remove {name}"), &[("name", m)]);
                        let r = ui.small_button(tr("Remove")).on_hover_text(&name);
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                        if r.clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    self.run(e, "musician.remove", json!({"musician": i}));
                }
                ui.horizontal(|ui| {
                    let label = ui.label(tr("New musician"));
                    ui.add(egui::TextEdit::singleline(&mut self.sheet_draft.1).desired_width(120.0))
                        .labelled_by(label.id);
                    let ok = !self.sheet_draft.1.trim().is_empty();
                    if ui.add_enabled(ok, egui::Button::new(tr("Add musician"))).clicked() {
                        let name = std::mem::take(&mut self.sheet_draft.1);
                        self.run(e, "musician.add", json!({"name": name}));
                    }
                });
            });
        ui.separator();
    }

    /// The marking tools over the pages: view or pen, highlighter, note; the colour; removing
    /// the last mark or all of them.
    fn mark_bar(&mut self, ui: &mut Ui, e: &mut Engine, song: &Song) {
        ui.horizontal_wrapped(|ui| {
            let edit = &mut self.mark_edit;
            let label = ui.label(tr("Mark"));
            for (tool, name, hint) in [
                (MarkTool::View, tr("Look"), tr("Look at the pages and scroll them")),
                (MarkTool::Pen, tr("Pen"), tr("Draw on the page with the pointer")),
                (MarkTool::Highlight, tr("Highlighter"), tr("Drag a box over what you want to stand out")),
                (MarkTool::Note, tr("Note"), tr("Click where a note goes, then write it")),
            ] {
                ui.selectable_value(&mut edit.tool, tool, name).on_hover_text(hint).labelled_by(label.id);
            }
            let label = ui.label(tr("Colour"));
            let current = MARK_COLOURS.get(edit.colour).map_or("", |c| c.0);
            egui::ComboBox::from_id_salt("mark-colour")
                .selected_text(tr(current))
                .show_ui(ui, |ui| {
                    for (i, (name, _)) in MARK_COLOURS.iter().enumerate() {
                        ui.selectable_value(&mut edit.colour, i, tr(name));
                    }
                })
                .response
                .labelled_by(label.id);
            ui.separator();
            // Without a pointer, a note goes on the first page in view.
            if ui
                .button(tr("Add note\u{2026}"))
                .on_hover_text(tr("Write a note at the top of the first page in view"))
                .clicked()
            {
                let page = u32::try_from(self.sheet_top_page).unwrap_or(0);
                self.note_prompt = Some((song.id, self.picked_sheet(song), page, [0.04, 0.04], String::new()));
            }
            let marks = song.pdf_marks.len();
            if ui
                .add_enabled(marks > 0, egui::Button::new(tr("Remove last mark")))
                .on_hover_text(tr("Take back the mark made last"))
                .clicked()
            {
                self.run(
                    e,
                    "pdf.unmark",
                    json!({"song": song.id, "sheet": self.picked_sheet(song), "mark": marks - 1}),
                );
            }
            if self.confirm_clear_marks && marks > 0 {
                let text = actcraft_engine::fill(tr("Remove all {count} marks?"), &[("count", &marks.to_string())]);
                ui.label(text);
                if ui.button(tr("Remove")).clicked() {
                    self.run(
                        e,
                        "pdf.clear_marks",
                        json!({"song": song.id, "sheet": self.picked_sheet(song), "confirm": true}),
                    );
                    self.confirm_clear_marks = false;
                }
                if ui.button(tr("Cancel")).clicked() {
                    self.confirm_clear_marks = false;
                }
            } else if ui.add_enabled(marks > 0, egui::Button::new(tr("Remove all marks\u{2026}"))).clicked() {
                self.confirm_clear_marks = true;
            }
        });
    }

    /// The small window that takes a note's words; Enter adds it, Esc or Cancel drops it.
    fn note_window(&mut self, ctx: &egui::Context, e: &mut Engine) {
        let Some((song, sheet, page, at, mut text)) = self.note_prompt.take() else { return };
        let mut keep = true;
        let mut add = false;
        egui::Modal::new(egui::Id::unique("pdf-note")).show(ctx, |ui| {
            ui.set_min_width(360.0);
            ui.heading(tr("Note"));
            let label =
                ui.label(actcraft_engine::fill(tr("Note on page {page}"), &[("page", &(page + 1).to_string())]));
            let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(320.0)).labelled_by(label.id);
            if !field.has_focus() && !field.lost_focus() {
                field.request_focus();
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                add = true;
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(!text.trim().is_empty(), egui::Button::new(tr("Add"))).clicked() {
                    add = true;
                }
                if ui.button(tr("Cancel")).clicked() {
                    keep = false;
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
        if add && !text.trim().is_empty() {
            let prm = json!({"song": song, "sheet": sheet, "page": page, "kind": "note", "points": [at], "text": text});
            self.run(e, "pdf.mark", prm);
            keep = false;
        }
        if keep {
            self.note_prompt = Some((song, sheet, page, at, text));
        }
    }
}

/// Opens `path` as the PDF in `slot` unless it already is (`None` closes it); the error says why
/// it could not be opened. A failed file is tried again on the next call.
pub(crate) fn load_pdf_into(
    slot: &mut Option<SheetDoc>,
    ctx: &egui::Context,
    e: &Engine,
    path: Option<&str>,
) -> Result<(), String> {
    if slot.as_ref().map(|s| s.path.as_str()) == path {
        return Ok(());
    }
    *slot = None;
    let Some(path) = path else { return Ok(()) };
    *slot = Some(e.services().read_file(path).and_then(|b| SheetDoc::open(path, b, ctx.clone()))?);
    Ok(())
}

/// What the pages did: a finished mark or a spot for a note, and the first page in view.
#[derive(Debug, Default)]
pub(crate) struct PagesOutput {
    pub request: Option<MarkRequest>,
    pub top_page: usize,
}

/// Draws a PDF's pages one under the other at `look`'s zoom, with the song's marks; with
/// `look.follow` they scroll at a constant speed over the song while it plays (`status` is the
/// time and whether it plays). With `edit`, the pointer marks the pages.
pub(crate) fn pdf_pages(
    ui: &mut Ui,
    doc: &mut SheetDoc,
    look: SheetLook,
    song: &Song,
    status: (f64, bool),
    salt: &'static str,
    mut edit: Option<&mut MarkEdit>,
) -> PagesOutput {
    let mut out = PagesOutput::default();
    let (time, playing) = status;
    doc.collect(ui.ctx());
    let available = ui.available_width();
    let shown_w = (available * look.zoom).max(100.0);
    let px = render_width(shown_w, ui.ctx().pixels_per_point());
    let gap = 12.0;
    let heights: Vec<f32> = doc.pages.iter().map(|&(w, h)| shown_w * h / w).collect();
    let content: f32 = heights.iter().sum::<f32>() + gap * heights.len() as f32;
    let mut area = egui::ScrollArea::both().id_salt((salt, song.id)).auto_shrink([false, false]);
    if look.follow && playing {
        let span = (song.start, song.end.unwrap_or(song.duration));
        if span.1 > span.0 {
            let frac = ((time - span.0) / (span.1 - span.0)).clamp(0.0, 1.0) as f32;
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
                let page = u32::try_from(i).unwrap_or(u32::MAX);
                paint_marks(ui.painter(), rect, song.pdf_marks.iter().filter(|m| m.page == page), paper);
                if let Some(edit) = edit.as_deref_mut().filter(|e| e.tool != MarkTool::View) {
                    let r = ui
                        .interact(rect, egui::Id::unique((salt, song.id, "page", i)), egui::Sense::click_and_drag())
                        .on_hover_cursor(egui::CursorIcon::Crosshair);
                    if let Some(req) = mark_input(edit, page, rect, &r) {
                        out.request = Some(req);
                    }
                    // The mark being drawn, as it will look.
                    if let Some((p, points)) = edit.draft.as_ref().filter(|d| d.0 == page) {
                        let kind =
                            if edit.tool == MarkTool::Highlight { PdfMarkKind::Highlight } else { PdfMarkKind::Pen };
                        let draft = PdfMark {
                            page: *p,
                            kind,
                            colour: edit.colour_hex().to_owned(),
                            points: points.clone(),
                            ..PdfMark::default()
                        };
                        paint_marks(ui.painter(), rect, std::iter::once(&draft), paper);
                    }
                }
            }
            if local.bottom() <= viewport.top() {
                out.top_page = (i + 1).min(heights.len().saturating_sub(1));
            }
            y += h + gap;
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives `mark_input` on a 200×100 page at the window's top left with pointer events, one
    /// list per frame; returns what it finished.
    fn drive(edit: &mut MarkEdit, frames: &[Vec<egui::Event>]) -> Vec<MarkRequest> {
        let ctx = egui::Context::default();
        let page = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(200.0, 100.0));
        let mut out = Vec::new();
        for (k, events) in frames.iter().enumerate() {
            let input = egui::RawInput {
                events: events.clone(),
                screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(400.0, 300.0))),
                time: Some(k as f64 * 0.1),
                ..Default::default()
            };
            let mut frame = ctx.run_ui(input, |ui| {
                let r = ui.interact(page, egui::Id::unique("test-page"), egui::Sense::click_and_drag());
                if let Some(req) = mark_input(edit, 3, page, &r) {
                    out.push(req);
                }
            });
            // No renderer takes the font texture here.
            frame.textures_delta.clear();
        }
        out
    }

    fn press(x: f32, y: f32, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    fn drag() -> Vec<Vec<egui::Event>> {
        let to = |x, y| egui::Event::PointerMoved(egui::pos2(x, y));
        vec![
            vec![to(20.0, 10.0)],
            vec![press(20.0, 10.0, true)],
            vec![to(60.0, 30.0)],
            vec![to(100.0, 50.0)],
            vec![press(100.0, 50.0, false)],
            vec![],
        ]
    }

    #[test]
    fn dragging_on_a_page_draws_a_stroke_or_a_box_and_a_click_asks_for_a_note() {
        let mut pen = MarkEdit { tool: MarkTool::Pen, colour: 1, draft: None };
        let got = drive(&mut pen, &drag());
        let [MarkRequest::Mark(prm)] = got.as_slice() else { panic!("one stroke: {got:?}") };
        assert_eq!(
            (prm["page"].clone(), prm["kind"].clone(), prm["colour"].clone()),
            (json!(3), json!("pen"), json!("#4f9cf0"))
        );
        let points = prm["points"].as_array().cloned().unwrap_or_default();
        assert!(points.len() >= 2, "{points:?}");
        assert_eq!(points.last(), Some(&json!([0.5, 0.5])), "fractions of the page");
        assert!(pen.draft.is_none(), "the draft is handed over");
        // A highlighter keeps only the two corners.
        let mut marker = MarkEdit { tool: MarkTool::Highlight, ..MarkEdit::default() };
        let got = drive(&mut marker, &drag());
        let [MarkRequest::Mark(prm)] = got.as_slice() else { panic!("one box: {got:?}") };
        assert_eq!(prm["points"].as_array().map(Vec::len), Some(2));
        // A click with the note tool gives the spot; looking does nothing.
        let click = vec![
            vec![egui::Event::PointerMoved(egui::pos2(50.0, 25.0))],
            vec![press(50.0, 25.0, true)],
            vec![press(50.0, 25.0, false)],
            vec![],
        ];
        let mut note = MarkEdit { tool: MarkTool::Note, ..MarkEdit::default() };
        assert_eq!(drive(&mut note, &click), vec![MarkRequest::NoteAt { page: 3, at: [0.25, 0.25] }]);
        let mut look = MarkEdit::default();
        assert!(drive(&mut look, &drag()).is_empty());
    }

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
