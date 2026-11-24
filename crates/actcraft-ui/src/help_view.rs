//! The help window (Help > ActCraft Help, F1): pages written in Markdown in the language folder,
//! with their pictures, in the window's language.
//!
//! Where the help lives (see `docs/translating.md`):
//!
//! - `i18n/help/contents.txt`: the pages in the order the contents list shows them, one name a
//!   line (lines starting with `#` are notes).
//! - `i18n/help/<language>/<page>.md`: a page; English in `en/`, German in `de/`, and so on. Its
//!   first `# ` line is its title.
//! - `i18n/help/img_<language>/<name>.png`: the pictures a page shows with a line of its own,
//!   `![What the picture shows](name.png)`.
//!
//! A page or picture a language doesn't have yet is shown in English. Links go to another page
//! (`[Mixer](songs-and-mixer)`), to a part of one (`songs-and-mixer#volume`, a heading's words in
//! lower case joined by `-`), or to a website (opened in the browser when clicked). The Markdown
//! is read with the shared `craft-markdown`; this window draws what it reads.

use std::collections::HashMap;

use craft_flow::{FBlock, Inline, Kind, Para};
use egui::{RichText, Ui};

use crate::{ActCraftApp, tr};

include!(concat!(env!("OUT_DIR"), "/help.rs"));

/// The page the help opens on when no other is asked for.
pub const START: &str = "getting-started";
/// Steps kept for Back.
const MAX_HISTORY: usize = 64;

/// The help window while it is open: the page shown, the part to scroll to, and the way back.
#[derive(Default)]
pub(crate) struct HelpState {
    page: String,
    anchor: Option<String>,
    /// Scroll to `anchor` on the next frame.
    jump: bool,
    back: Vec<(String, Option<String>)>,
    forward: Vec<(String, Option<String>)>,
    pictures: HashMap<(String, String), egui::TextureHandle>,
}

/// A help page's text in `lang`, or in English when that language has none.
pub fn page_text(lang: &str, page: &str) -> Option<&'static str> {
    let find = |l: &str| HELP_PAGES.iter().find(|(pl, name, _)| *pl == l && *name == page).map(|(_, _, t)| *t);
    find(lang).or_else(|| find("en"))
}

/// A help picture's PNG bytes in `lang`, or the English one.
fn picture(lang: &str, name: &str) -> Option<(&'static str, &'static [u8])> {
    let find = |l: &str| HELP_IMAGES.iter().find(|(pl, n, _)| *pl == l && *n == name);
    find(lang).or_else(|| find("en")).map(|(pl, _, b)| (*pl, *b))
}

/// The pages in contents order (then any page the order leaves out).
pub fn pages() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = HELP_ORDER.to_vec();
    for (_, name, _) in HELP_PAGES {
        if !out.contains(name) {
            out.push(name);
        }
    }
    out
}

/// A page's title: its first `# ` line, else its name.
pub fn title(text: &str, page: &str) -> String {
    text.lines().find_map(|l| l.strip_prefix("# ")).map_or_else(|| page.to_owned(), |t| t.trim().to_owned())
}

/// The anchor of a heading: its words in lower case joined by `-`.
pub fn slug(heading: &str) -> String {
    let mut out = String::new();
    for c in heading.trim().chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if (c.is_whitespace() || c == '-') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

/// A link target: another page and a part of it, a part of this page (`#part`), or a website.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Page(String, Option<String>),
    Web(String),
}

pub fn target(link: &str, current: &str) -> Target {
    if link.starts_with("http://") || link.starts_with("https://") {
        return Target::Web(link.to_owned());
    }
    let link = link.trim_end_matches(".md");
    match link.split_once('#') {
        Some(("", anchor)) => Target::Page(current.to_owned(), Some(anchor.to_owned())),
        Some((page, anchor)) => Target::Page(page.trim_end_matches(".md").to_owned(), Some(anchor.to_owned())),
        None => Target::Page(link.to_owned(), None),
    }
}

/// A page cut into text and the pictures on lines of their own.
#[derive(Debug, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Picture { alt: String, file: String },
}

pub fn pieces(text: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut chunk = String::new();
    for line in text.lines() {
        let l = line.trim();
        let pic = l
            .strip_prefix("![")
            .and_then(|rest| rest.strip_suffix(')'))
            .and_then(|rest| rest.split_once("]("))
            .filter(|(_, file)| !file.contains(' ') && !file.is_empty());
        match pic {
            Some((alt, file)) => {
                if !chunk.trim().is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut chunk)));
                }
                chunk.clear();
                let file = file.rsplit('/').next().unwrap_or(file).trim_end_matches(".png").to_owned();
                out.push(Piece::Picture { alt: alt.to_owned(), file });
            }
            None => {
                chunk.push_str(line);
                chunk.push('\n');
            }
        }
    }
    if !chunk.trim().is_empty() {
        out.push(Piece::Text(chunk));
    }
    out
}

/// A paragraph's words, without formatting.
fn plain(p: &Para) -> String {
    p.inlines
        .iter()
        .map(|i| match i {
            Inline::Text(t, _) => t.as_str(),
            _ => "",
        })
        .collect()
}

impl ActCraftApp {
    /// Opens the help at `target` ("page" or "page#part"), keeping the page shown for Back.
    pub(crate) fn open_help(&mut self, target_link: &str) {
        let current = self.help.as_ref().map(|h| h.page.clone()).unwrap_or_default();
        let Target::Page(page, anchor) = target(target_link, &current) else { return };
        let state = self.help.get_or_insert_with(HelpState::default);
        if !state.page.is_empty() && (state.page != page || state.anchor != anchor) {
            state.back.push((state.page.clone(), state.anchor.clone()));
            if state.back.len() > MAX_HISTORY {
                state.back.remove(0);
            }
            state.forward.clear();
        }
        state.page = if page_text("en", &page).is_some() { page } else { START.to_owned() };
        state.anchor = anchor;
        state.jump = true;
    }

    /// Opens the help where it was left, or at the start.
    pub(crate) fn show_help(&mut self) {
        if self.help.is_none() {
            self.open_help(START);
        }
    }

    /// The help window, while it is open.
    pub(crate) fn help_window(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.help.take() else { return };
        let lang = crate::i18n::current_code();
        let t = self.tokens;
        let mut open = true;
        let mut go: Option<String> = None;
        let mut web: Option<String> = None;
        let mut step: Option<bool> = None;
        egui::Window::new(tr("ActCraft Help"))
            .open(&mut open)
            .default_width(820.0)
            .default_height(600.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled(!state.back.is_empty(), egui::Button::new(tr("Back"))).clicked() {
                        step = Some(false);
                    }
                    if ui.add_enabled(!state.forward.is_empty(), egui::Button::new(tr("Forward"))).clicked() {
                        step = Some(true);
                    }
                });
                ui.separator();
                ui.horizontal_top(|ui| {
                    // The contents: every page by its title.
                    ui.vertical(|ui| {
                        ui.set_width(190.0);
                        ui.label(RichText::new(tr("Contents")).strong());
                        egui::ScrollArea::vertical().id_salt("help-contents").show(ui, |ui| {
                            for page in pages() {
                                let text = page_text(&lang, page).unwrap_or_default();
                                let here = state.page == page;
                                if ui.selectable_label(here, title(text, page)).clicked() && !here {
                                    go = Some(page.to_owned());
                                }
                            }
                        });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical().id_salt(("help-page", state.page.clone())).show(ui, |ui| {
                            ui.set_max_width(560.0);
                            let text = page_text(&lang, &state.page).unwrap_or_default();
                            for piece in pieces(text) {
                                match piece {
                                    Piece::Text(chunk) => {
                                        let flow = craft_markdown::parse(&chunk);
                                        draw_blocks(ui, t, &flow.blocks, &mut state, &mut go, &mut web);
                                    }
                                    Piece::Picture { alt, file } => {
                                        draw_picture(ui, &lang, &alt, &file, &mut state.pictures);
                                    }
                                }
                            }
                        });
                    });
                });
            });
        state.jump = false;
        if let Some(forward) = step {
            let (from, to) =
                if forward { (&mut state.forward, &mut state.back) } else { (&mut state.back, &mut state.forward) };
            if let Some((page, anchor)) = from.pop() {
                to.push((state.page.clone(), state.anchor.clone()));
                state.page = page;
                state.anchor = anchor;
                state.jump = true;
            }
        }
        if let Some(url) = web {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        if open && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.help = Some(state);
            if let Some(link) = go {
                self.open_help(&link);
            }
        }
    }
}

/// Draws one picture of a page, scaled to fit, with its words for screen readers.
fn draw_picture(
    ui: &mut Ui,
    lang: &str,
    alt: &str,
    file: &str,
    cache: &mut HashMap<(String, String), egui::TextureHandle>,
) {
    let Some((found, bytes)) = picture(lang, file) else {
        ui.label(RichText::new(alt).italics());
        return;
    };
    let key = (found.to_owned(), file.to_owned());
    if !cache.contains_key(&key) {
        let Ok(img) = image::load_from_memory_with_format(bytes, image::ImageFormat::Png) else {
            ui.label(RichText::new(alt).italics());
            return;
        };
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let colour = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let tex = ui.ctx().load_texture(format!("help-{found}-{file}"), colour, egui::TextureOptions::LINEAR);
        cache.insert(key.clone(), tex);
    }
    if let Some(tex) = cache.get(&key) {
        let size = tex.size_vec2();
        let width = ui.available_width().min(size.x);
        let shown = egui::vec2(width, size.y * width / size.x.max(1.0));
        ui.add(egui::Image::new((tex.id(), shown)).alt_text(alt));
        ui.add_space(6.0);
    }
}

/// Draws the paragraphs and tables a page's Markdown was read into.
fn draw_blocks(
    ui: &mut Ui,
    t: crate::theme::Tokens,
    blocks: &[FBlock],
    state: &mut HelpState,
    go: &mut Option<String>,
    web: &mut Option<String>,
) {
    let mut numbers: Vec<u32> = Vec::new();
    for block in blocks {
        match block {
            FBlock::Para(p) => {
                // Numbers of ordered lists, by level.
                match p.list {
                    Some(l) => {
                        let level = usize::from(l.level);
                        numbers.truncate(level + 1);
                        while numbers.len() <= level {
                            numbers.push(l.start.saturating_sub(1));
                        }
                    }
                    None => numbers.clear(),
                }
                draw_para(ui, t, p, &mut numbers, state, go, web);
            }
            FBlock::Table(table) => {
                egui::Grid::new(("help-table", ui.next_auto_id())).striped(true).show(ui, |ui| {
                    for row in &table.rows {
                        for cell in row {
                            let words: Vec<String> = cell
                                .blocks
                                .iter()
                                .filter_map(|b| if let FBlock::Para(p) = b { Some(plain(p)) } else { None })
                                .collect();
                            let text = words.join(" ");
                            if cell.header {
                                ui.label(RichText::new(text).strong())
                            } else {
                                ui.label(text)
                            };
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
            }
        }
    }
}

fn draw_para(
    ui: &mut Ui,
    t: crate::theme::Tokens,
    p: &Para,
    numbers: &mut [u32],
    state: &mut HelpState,
    go: &mut Option<String>,
    web: &mut Option<String>,
) {
    match p.kind {
        Kind::Heading(_) | Kind::Title if !plain(p).trim().is_empty() => {
            let level = if let Kind::Heading(n) = p.kind { n } else { 1 };
            let words = plain(p);
            let size = match level {
                1 => 24.0,
                2 => 19.0,
                _ => 16.0,
            };
            ui.add_space(if level == 1 { 4.0 } else { 10.0 });
            let r = ui.label(RichText::new(&words).size(size).strong().color(t.text));
            if state.jump && state.anchor.as_deref() == Some(slug(&words).as_str()) {
                r.scroll_to_me(Some(egui::Align::TOP));
            }
            return;
        }
        Kind::Rule => {
            ui.separator();
            return;
        }
        Kind::Code => {
            ui.label(RichText::new(plain(p)).monospace().color(t.text));
            return;
        }
        _ => {}
    }
    let quote = matches!(p.kind, Kind::Quote);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if let Some(l) = p.list {
            ui.add_space(14.0 + 16.0 * f32::from(l.level));
            let mark = if l.ordered {
                let level = usize::from(l.level);
                let n = numbers.get_mut(level).map_or(1, |n| {
                    *n += 1;
                    *n
                });
                format!("{n}. ")
            } else {
                "\u{2022} ".to_owned()
            };
            ui.label(mark);
        }
        if quote {
            ui.add_space(12.0);
        }
        for inline in &p.inlines {
            let Inline::Text(text, fmt) = inline else { continue };
            if let Some(link) = &fmt.link {
                if ui.link(text.as_str()).clicked() {
                    match target(link, &state.page) {
                        Target::Page(page, anchor) => {
                            *go = Some(match anchor {
                                Some(a) => format!("{page}#{a}"),
                                None => page,
                            });
                        }
                        Target::Web(url) => *web = Some(url),
                    }
                }
                continue;
            }
            let mut rich = RichText::new(text.as_str()).color(if quote { t.text_dim } else { t.text });
            if fmt.bold {
                rich = rich.strong();
            }
            if fmt.italic || quote {
                rich = rich.italics();
            }
            if fmt.code {
                rich = rich.monospace();
            }
            ui.label(rich);
        }
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_slugs_and_pictures_are_read() {
        assert_eq!(slug("Volume & Mute groups"), "volume-mute-groups");
        assert_eq!(slug("  DMX-Ausgang über USB "), "dmx-ausgang-über-usb");
        assert_eq!(target("songs#volume", "x"), Target::Page("songs".into(), Some("volume".into())));
        assert_eq!(target("#volume", "here"), Target::Page("here".into(), Some("volume".into())));
        assert_eq!(target("lighting.md", "x"), Target::Page("lighting".into(), None));
        assert_eq!(target("https://actcraft.org", "x"), Target::Web("https://actcraft.org".into()));
        let p = pieces("# Title\n\nText\n![The mixer](img/mixer.png)\nMore\n");
        assert_eq!(
            p,
            vec![
                Piece::Text("# Title\n\nText\n".into()),
                Piece::Picture { alt: "The mixer".into(), file: "mixer".into() },
                Piece::Text("More\n".into()),
            ]
        );
        assert_eq!(title("Intro\n# Getting started\n", "x"), "Getting started");
    }

    #[test]
    fn every_page_in_the_contents_exists_in_english() {
        assert!(!HELP_ORDER.is_empty());
        for page in HELP_ORDER {
            assert!(page_text("en", page).is_some(), "{page} has no English page");
        }
        // Every language's page is one English has too, and every picture a page shows exists.
        for (lang, page, text) in HELP_PAGES {
            assert!(page_text("en", page).is_some(), "{lang}/{page} has no English page");
            for piece in pieces(text) {
                if let Piece::Picture { file, .. } = piece {
                    assert!(picture(lang, &file).is_some(), "{lang}/{page} shows {file}.png, which is missing");
                }
            }
        }
    }
}
