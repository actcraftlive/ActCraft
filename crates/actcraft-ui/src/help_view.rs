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
    text.lines().find_map(|l| l.strip_prefix("# ")).map_or_else(|| page.to_owned(), |t| heading(t).0.to_owned())
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

/// A heading's words and its anchor. `## Words {#id}` gives the id, which a translation keeps, so
/// links and **?** buttons find the part in every language. A heading without one answers to its
/// words joined by `-` ([`slug`]).
pub fn heading(words: &str) -> (&str, String) {
    let w = words.trim();
    let id = w.strip_suffix('}').and_then(|rest| rest.rsplit_once("{#")).filter(|(_, id)| {
        !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    });
    match id {
        Some((shown, id)) => (shown.trim_end(), id.to_owned()),
        None => (w, slug(w)),
    }
}

/// The parts of the help a **?** button opens. Each names a page and, for most, a part of it by
/// its `{#id}`; a test checks every one is there in every language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Topic {
    SongDetails,
    Mixer,
    SpokenCues,
    Lyrics,
    SheetMusic,
    SheetsPerMusician,
    SheetsInBrowser,
    Midi,
    RemoteActions,
    Pads,
    Macros,
    Library,
    DmxOutput,
    DmxIn,
    DmxMonitor,
    Fixtures,
    FixtureTypes,
    StageView,
    Presets,
    CueLists,
    Effects,
    Sequences,
    Hits,
    BandLink,
}

impl Topic {
    #[cfg(test)]
    pub(crate) const ALL: [Topic; 24] = [
        Topic::SongDetails,
        Topic::Mixer,
        Topic::SpokenCues,
        Topic::Lyrics,
        Topic::SheetMusic,
        Topic::SheetsPerMusician,
        Topic::SheetsInBrowser,
        Topic::Midi,
        Topic::RemoteActions,
        Topic::Pads,
        Topic::Macros,
        Topic::Library,
        Topic::DmxOutput,
        Topic::DmxIn,
        Topic::DmxMonitor,
        Topic::Fixtures,
        Topic::FixtureTypes,
        Topic::StageView,
        Topic::Presets,
        Topic::CueLists,
        Topic::Effects,
        Topic::Sequences,
        Topic::Hits,
        Topic::BandLink,
    ];

    /// Where in the help it is: "page" or "page#id".
    pub(crate) fn link(self) -> &'static str {
        match self {
            Topic::SongDetails => "songs-and-mixer#details",
            Topic::Mixer => "songs-and-mixer#mixer",
            Topic::SpokenCues => "songs-and-mixer#spoken-cues",
            Topic::Lyrics => "lyrics-and-chords",
            Topic::SheetMusic => "sheet-music",
            Topic::SheetsPerMusician => "sheet-music#per-musician",
            Topic::SheetsInBrowser => "sheet-music#browser",
            Topic::Midi => "midi-and-remote",
            Topic::RemoteActions => "midi-and-remote#remote-actions",
            Topic::Pads => "midi-and-remote#pads",
            Topic::Macros => "midi-and-remote#macros",
            Topic::Library => "getting-started#first-show",
            Topic::DmxOutput => "lighting-output",
            Topic::DmxIn => "lighting-output#dmx-in",
            Topic::DmxMonitor => "lighting-output#levels",
            Topic::Fixtures => "fixtures",
            Topic::FixtureTypes => "fixtures#own-types",
            Topic::StageView => "fixtures#stage-view",
            Topic::Presets => "looks-and-cues#presets",
            Topic::CueLists => "looks-and-cues#cue-lists",
            Topic::Effects => "looks-and-cues#effects",
            Topic::Sequences => "looks-and-cues#sequences",
            Topic::Hits => "looks-and-cues#hits",
            Topic::BandLink => "band-link",
        }
    }
}

/// The words of a help link's part (its heading) or page (its title), in `lang`, or `None` when
/// the page or part isn't there.
fn topic_words(lang: &str, link: &str) -> Option<String> {
    let Target::Page(page, anchor) = target(link, "") else { return None };
    let text = page_text(lang, &page)?;
    match anchor {
        None => Some(title(text, &page)),
        Some(a) => text.lines().filter(|l| l.starts_with('#')).find_map(|l| {
            let (shown, id) = heading(l.trim_start_matches('#'));
            (id == a).then(|| shown.to_owned())
        }),
    }
}

/// Where a **?** button leaves its request for the help window.
fn request_id() -> egui::Id {
    egui::Id::unique("actcraft-help-request")
}

/// A small **?** button that opens the help at `topic`. Its tooltip and its name for screen readers
/// say which part: "Help: Sequences".
pub(crate) fn help_button(ui: &mut Ui, topic: Topic) {
    let lang = crate::i18n::current_code();
    let words = topic_words(&lang, topic.link()).unwrap_or_default();
    let what = format!("{} {words}", tr("Help:"));
    let r = ui.small_button("?").on_hover_text(&what);
    let name = format!("? {what}");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    if r.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(request_id(), topic.link().to_owned()));
        ui.ctx().request_repaint();
    }
}

/// A section's bold title with its **?** button beside it.
pub(crate) fn section_title(ui: &mut Ui, words: &str, topic: Topic) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(words).strong());
        help_button(ui, topic);
    });
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
        // A **?** button pressed somewhere in the window.
        if let Some(link) = ctx.data_mut(|d| d.remove_temp::<String>(request_id())) {
            self.open_help(&link);
        }
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
            let (shown, anchor) = heading(&words);
            let size = match level {
                1 => 24.0,
                2 => 19.0,
                _ => 16.0,
            };
            ui.add_space(if level == 1 { 4.0 } else { 10.0 });
            let r = ui.label(RichText::new(shown).size(size).strong().color(t.text));
            if state.jump && state.anchor.as_deref() == Some(anchor.as_str()) {
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
            // Bold names what to press or choose: strong text. Not the accent colour: that is the
            // colour of links, and bold words are not links.
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

    /// Every link of every page, in every language, leads to a page there is and, for a part, to
    /// a heading that page (in that language) has.
    #[test]
    fn every_link_leads_somewhere() {
        let headings = |lang: &str, page: &str| -> Vec<String> {
            let text = page_text(lang, page).unwrap_or_default();
            craft_markdown::parse(text)
                .blocks
                .iter()
                .filter_map(|b| match b {
                    FBlock::Para(p) if matches!(p.kind, Kind::Heading(_) | Kind::Title) => Some(heading(&plain(p)).1),
                    _ => None,
                })
                .collect()
        };
        for (lang, page, text) in HELP_PAGES {
            for block in craft_markdown::parse(text).blocks {
                let FBlock::Para(p) = block else { continue };
                for inline in &p.inlines {
                    let Inline::Text(_, fmt) = inline else { continue };
                    let Some(link) = &fmt.link else { continue };
                    if let Target::Page(to, anchor) = target(link, page) {
                        assert!(page_text("en", &to).is_some(), "{lang}/{page} links to {to}, which is no page");
                        if let Some(a) = anchor {
                            assert!(
                                headings(lang, &to).contains(&a),
                                "{lang}/{page} links to {to}#{a}, which is no heading"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_heading_keeps_its_id_in_every_language() {
        assert_eq!(heading("The mixer {#mixer}"), ("The mixer", "mixer".to_owned()));
        assert_eq!(heading(" Der Mixer {#mixer} "), ("Der Mixer", "mixer".to_owned()));
        assert_eq!(heading("Spoken cues"), ("Spoken cues", "spoken-cues".to_owned()));
        // Not an id: spaces or capitals inside, or nothing.
        assert_eq!(heading("Sets {#a b}").1, "sets-a-b");
        assert_eq!(heading("Sets {#}").1, "sets");
        // Markdown keeps `{#id}` as words of the heading, so the window can take it off.
        let flow = craft_markdown::parse("## Der Mixer {#mixer}\n");
        let words = flow.blocks.iter().find_map(|b| if let FBlock::Para(p) = b { Some(plain(p)) } else { None });
        assert_eq!(words.as_deref().map(heading).map(|h| h.1), Some("mixer".to_owned()));
        assert_eq!(title("# Updates {#updates}\n", "updates"), "Updates");
    }

    /// Every **?** button's part is there in every language the help has, so a button never opens
    /// the wrong place.
    #[test]
    fn every_question_mark_finds_its_part_in_every_language() {
        let mut langs: Vec<&str> = HELP_PAGES.iter().map(|(lang, _, _)| *lang).collect();
        langs.dedup();
        for lang in langs {
            for topic in Topic::ALL {
                assert!(
                    topic_words(lang, topic.link()).is_some(),
                    "{lang}: {:?} ({}) is not in the help",
                    topic,
                    topic.link()
                );
            }
        }
        assert_eq!(
            topic_words("de", Topic::Sequences.link()).as_deref(),
            Some("Abläufe: Animationen für Moving Heads")
        );
        assert_eq!(topic_words("en", Topic::BandLink.link()).as_deref(), Some("Band link"));
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
