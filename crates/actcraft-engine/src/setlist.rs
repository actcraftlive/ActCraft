//! The printed setlist: a playlist's sets as a PDF to tape to the stage floor, with the band's
//! logo and the concert's name at the top, each song's number, title and length, the lines set
//! before songs (a break, an instrument change) and the time of each set and of all of them.
//! It carries no words of its own, so it reads the same in any language.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::Show;
use actcraft_show::limits::MAX_TEXT;
use serde_json::{Value, json};

use crate::{Engine, services};

/// Longest line set before a song.
const MAX_NOTE: usize = 200;
/// Largest logo file read.
const MAX_LOGO_BYTES: usize = 8 << 20;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// One line of the setlist.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// A set's name and its songs' time together (seconds).
    Set { name: String, length: f64 },
    /// A line before a song.
    Note(String),
    /// A song: its number in the playlist, title and length (0 when not known yet).
    Song { number: usize, title: String, length: f64 },
}

/// The setlist of playlist `index`: its lines and the time of all songs (seconds). A set's name
/// is left out when the playlist has one set only.
pub fn entries(show: &Show, index: usize) -> Option<(Vec<Entry>, f64)> {
    let playlist = show.playlists.get(index)?;
    let mut out = Vec::new();
    let mut total = 0.0;
    let mut number = 0;
    let named = playlist.sets.len() > 1;
    for set in &playlist.sets {
        let lengths: Vec<f64> = set.songs.iter().map(|id| show.song(*id).map_or(0.0, |s| s.duration)).collect();
        let length: f64 = lengths.iter().sum();
        total += length;
        if named {
            out.push(Entry::Set { name: set.name.clone(), length });
        }
        for (id, length) in set.songs.iter().zip(lengths) {
            if let Some(note) = set.notes.iter().find(|n| n.song == *id && !n.text.trim().is_empty()) {
                out.push(Entry::Note(note.text.trim().to_owned()));
            }
            number += 1;
            let title = show.song(*id).map(|s| s.title.trim().to_owned()).unwrap_or_default();
            out.push(Entry::Song { number, title, length });
        }
    }
    Some((out, total))
}

/// `m:ss`, or `h:mm:ss` from an hour; empty for an unknown (zero) length.
pub fn clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return String::new();
    }
    let s = seconds.round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn playlist_index(e: &Engine, prm: &Value) -> Result<usize, CommandError> {
    match p::opt_i64_in(prm, "playlist", 0, i64::MAX)? {
        Some(i) => {
            let i = usize::try_from(i).map_err(|_| invalid("bad playlist"))?;
            if i < e.show.playlists.len() { Ok(i) } else { Err(invalid("no such playlist")) }
        }
        None => e.current_playlist.filter(|&i| i < e.show.playlists.len()).ok_or_else(|| invalid("no playlist open")),
    }
}

/// `set.note {playlist?, set, song, text}`: the line printed before a song of a set; empty text
/// takes it away.
pub fn set_note(e: &mut Engine, prm: &Value) -> CommandResult {
    let playlist = playlist_index(e, prm)?;
    let sets = e.show.playlists.get(playlist).map_or(0, |pl| pl.sets.len());
    let set = p::index(prm, "set", sets)?;
    let song = u64::try_from(p::i64_in(prm, "song", 1, i64::MAX)?).map_err(|_| invalid("bad song id"))?;
    let text = p::str_max(prm, "text", MAX_NOTE)?.trim().to_owned();
    let Some(s) = e.show.playlists.get_mut(playlist).and_then(|pl| pl.sets.get_mut(set)) else {
        return Err(invalid("no such set"));
    };
    if !s.songs.contains(&song) {
        return Err(invalid("that song is not in the set"));
    }
    s.notes.retain(|n| n.song != song);
    if !text.is_empty() {
        s.notes.push(actcraft_show::SetNote { song, text, ..Default::default() });
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `setlist.logo {path|null}`: the band's logo for printed setlists (PNG or JPEG).
pub fn setlist_logo(e: &mut Engine, prm: &Value) -> CommandResult {
    if prm.get("path").is_none_or(Value::is_null) {
        e.show.band_logo = None;
    } else {
        let path = p::str_max(prm, "path", MAX_TEXT)?;
        if !matches!(services::extension(path).as_deref(), Some("png" | "jpg" | "jpeg")) {
            return Err(invalid("the logo must be a .png or .jpg file"));
        }
        e.show.band_logo = Some(path.to_owned());
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `playlist.export_setlist {path, playlist?, title?}` -> {pages, songs, length}: the setlist as
/// a PDF; the title is the concert's name (the playlist's name when left out).
pub fn playlist_export_setlist(e: &mut Engine, prm: &Value) -> CommandResult {
    let path = p::str_max(prm, "path", MAX_TEXT)?.to_owned();
    if services::extension(&path).as_deref() != Some("pdf") {
        return Err(invalid("a setlist is written as a .pdf file"));
    }
    let index = playlist_index(e, prm)?;
    let title = match p::opt_str_max(prm, "title", MAX_TEXT)? {
        Some(t) => t.trim().to_owned(),
        None => e.show.playlists.get(index).map(|pl| pl.name.clone()).unwrap_or_default(),
    };
    let Some((lines, length)) = entries(&e.show, index) else { return Err(invalid("no such playlist")) };
    let songs = lines.iter().filter(|l| matches!(l, Entry::Song { .. })).count();
    if songs == 0 {
        return Err(failed("the playlist has no songs to print"));
    }
    // A logo that can't be read leaves its place empty rather than failing the setlist.
    let logo =
        e.show.band_logo.clone().and_then(|p| e.services().read_file(&p).ok()).filter(|b| b.len() <= MAX_LOGO_BYTES);
    let (bytes, pages) = pdf::write(&title, logo, &lines, length).map_err(failed)?;
    e.services_mut().write_file(&path, &bytes).map_err(failed)?;
    e.say_with(
        "Setlist written: {songs} songs on {pages} pages",
        vec![crate::MessageArg::plain("songs", songs), crate::MessageArg::plain("pages", pages)],
    );
    Ok(json!({"pages": pages, "songs": songs, "length": (length * 10.0).round() / 10.0}))
}

/// Where the setlist's lines go on A4 paper, in points from the top left.
pub mod layout {
    use super::Entry;

    pub const PAGE: (f32, f32) = (595.28, 841.89);
    pub const MARGIN: f32 = 36.0;
    /// Song lines are this large when everything fits, and shrink to `SMALLEST` before taking a
    /// second page.
    pub const LARGEST: f32 = 40.0;
    pub const SMALLEST: f32 = 14.0;

    /// How tall a line is at song size `size`.
    pub fn height(entry: &Entry, size: f32) -> f32 {
        match entry {
            Entry::Song { .. } => size * 1.4,
            Entry::Note(_) => size * 0.75 * 1.4,
            Entry::Set { .. } => size * 0.8 * 1.9,
        }
    }

    /// The song size and the lines of each page, given the room under the header on the first
    /// page (`first`) and on the others (`rest`), and the total line at the end.
    pub fn pages(entries: &[Entry], first: f32, rest: f32) -> (f32, Vec<Vec<usize>>) {
        let total_line = |size: f32| size * 1.6;
        let mut size = LARGEST;
        loop {
            let need: f32 = entries.iter().map(|e| height(e, size)).sum::<f32>() + total_line(size);
            if need <= first || size <= SMALLEST {
                break;
            }
            size -= 1.0;
        }
        let mut pages = vec![Vec::new()];
        let mut room = first - total_line(size);
        for (i, entry) in entries.iter().enumerate() {
            let h = height(entry, size);
            let page_has_lines = pages.last().is_some_and(|p| !p.is_empty());
            if h > room && page_has_lines {
                pages.push(Vec::new());
                room = rest - total_line(size);
            }
            room -= h;
            if let Some(page) = pages.last_mut() {
                page.push(i);
            }
        }
        (size, pages)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod pdf {
    use craft_pdf::write::backend as krilla;
    use craft_pdf::write::{self, Metadata, Writer};
    use krilla::color::rgb;
    use krilla::geom::{Point, Size, Transform};
    use krilla::image::Image;
    use krilla::page::PageSettings;
    use krilla::paint::{Fill, Stroke};
    use krilla::surface::Surface;
    use krilla::text::{Font, TextDirection};
    use skrifa::MetadataProvider;
    use skrifa::instance::LocationRef;

    use super::layout::{self, MARGIN, PAGE};
    use super::{Entry, clock};

    /// The window's own font, so the paper looks like the program.
    const FONT: &[u8] = epaint_default_fonts::UBUNTU_LIGHT;
    const LOGO_HEIGHT: f32 = 90.0;
    const TITLE_SIZE: f32 = 32.0;

    /// Text widths from the font's advances (no kerning: close enough to place and cut lines).
    struct Measure<'a> {
        font: skrifa::FontRef<'a>,
    }

    impl Measure<'_> {
        fn width(&self, text: &str, size: f32) -> f32 {
            let map = self.font.charmap();
            let metrics = self.font.glyph_metrics(skrifa::instance::Size::new(size), LocationRef::default());
            text.chars().map(|c| map.map(c).and_then(|g| metrics.advance_width(g)).unwrap_or(size * 0.5)).sum()
        }

        /// `text` cut with an ellipsis to fit `room` points.
        fn fit(&self, text: &str, size: f32, room: f32) -> String {
            if self.width(text, size) <= room {
                return text.to_owned();
            }
            let mut out: String = text.to_owned();
            while !out.is_empty() && self.width(&format!("{out}\u{2026}"), size) > room {
                out.pop();
            }
            format!("{}\u{2026}", out.trim_end())
        }
    }

    struct Pen<'s, 'p> {
        surface: &'s mut Surface<'p>,
        font: Font,
    }

    impl Pen<'_, '_> {
        /// Text with its baseline at `y`, in grey `shade` (0 black), drawn a little heavier than
        /// the light font is (filled and outlined) so it reads from a distance.
        fn text(&mut self, x: f32, y: f32, size: f32, text: &str, shade: u8) {
            let colour = rgb::Color::new(shade, shade, shade);
            self.surface.set_fill(Some(Fill { paint: colour.into(), ..Fill::default() }));
            self.surface.set_stroke(None);
            self.surface.draw_text(Point::from_xy(x, y), self.font.clone(), size, text, false, TextDirection::Auto);
            self.surface.set_fill(None);
            self.surface.set_stroke(Some(Stroke { paint: colour.into(), width: size * 0.035, ..Stroke::default() }));
            self.surface.draw_text(Point::from_xy(x, y), self.font.clone(), size, text, true, TextDirection::Auto);
            self.surface.set_stroke(None);
        }

        fn rule(&mut self, y: f32) {
            let mut path = krilla::geom::PathBuilder::new();
            path.move_to(MARGIN, y);
            path.line_to(PAGE.0 - MARGIN, y);
            if let Some(path) = path.finish() {
                let grey = rgb::Color::new(120, 120, 120);
                self.surface.set_fill(None);
                self.surface.set_stroke(Some(Stroke { paint: grey.into(), width: 0.8, ..Stroke::default() }));
                self.surface.draw_path(&path);
                self.surface.set_stroke(None);
            }
        }
    }

    fn logo_image(bytes: Vec<u8>) -> Option<Image> {
        let png = bytes.starts_with(b"\x89PNG");
        let data: krilla::Data = bytes.into();
        if png { Image::from_png(data, true).ok() } else { Image::from_jpeg(data, true).ok() }
    }

    /// The setlist as PDF bytes and its page count.
    pub fn write(
        title: &str,
        logo: Option<Vec<u8>>,
        entries: &[Entry],
        total: f64,
    ) -> Result<(Vec<u8>, usize), String> {
        let measure =
            Measure { font: skrifa::FontRef::new(FONT).map_err(|_| "the font could not be read".to_owned())? };
        let font = write::font(FONT, 0, None).map_err(|e| e.to_string())?;
        let logo = logo.and_then(logo_image);
        // The header: the logo, then the concert's name.
        let mut top = MARGIN;
        if logo.is_some() {
            top += LOGO_HEIGHT + 12.0;
        }
        if !title.is_empty() {
            top += TITLE_SIZE * 1.6;
        }
        let bottom = PAGE.1 - MARGIN;
        let (size, pages) = layout::pages(entries, bottom - top, bottom - MARGIN);
        let count = pages.len();
        let metadata = Metadata {
            creator: "ActCraft".to_owned(),
            producer: "ActCraft".to_owned(),
            title: (!title.is_empty()).then(|| title.to_owned()),
            ..Metadata::default()
        };
        let bytes = write::guard(|| {
            let mut writer = Writer::new(krilla::SerializeSettings::default(), metadata);
            for (n, lines) in pages.iter().enumerate() {
                let settings = PageSettings::from_wh(PAGE.0, PAGE.1).ok_or(write::WriteError::Allocation)?;
                let mut page = writer.document_mut().start_page_with(settings);
                let mut surface = page.surface();
                let mut pen = Pen { surface: &mut surface, font: font.clone() };
                let mut y = MARGIN;
                if n == 0 {
                    if let Some(image) = &logo {
                        let (w, h) = image.size();
                        let scale = LOGO_HEIGHT / h.max(1) as f32;
                        let width = (w as f32 * scale).min(PAGE.0 - 2.0 * MARGIN);
                        let height = width / w.max(1) as f32 * h as f32;
                        if let Some(sz) = Size::from_wh(width, height) {
                            pen.surface.push_transform(&Transform::from_translate((PAGE.0 - width) / 2.0, y));
                            pen.surface.draw_image(image.clone(), sz);
                            pen.surface.pop();
                        }
                        y += LOGO_HEIGHT + 12.0;
                    }
                    if !title.is_empty() {
                        let text = measure.fit(title, TITLE_SIZE, PAGE.0 - 2.0 * MARGIN);
                        let x = (PAGE.0 - measure.width(&text, TITLE_SIZE)) / 2.0;
                        pen.text(x, y + TITLE_SIZE, TITLE_SIZE, &text, 0);
                        y += TITLE_SIZE * 1.6;
                    }
                }
                let number_room = measure.width("00", size) + size * 0.5;
                let time_room = measure.width("00:00", size * 0.8) + size;
                for &i in lines {
                    let Some(entry) = entries.get(i) else { continue };
                    let h = layout::height(entry, size);
                    match entry {
                        Entry::Set { name, length } => {
                            let s = size * 0.8;
                            pen.text(
                                MARGIN,
                                y + h - s * 0.5,
                                s,
                                &measure.fit(name, s, PAGE.0 - 2.0 * MARGIN - time_room),
                                60,
                            );
                            let t = clock(*length);
                            pen.text(PAGE.0 - MARGIN - measure.width(&t, s), y + h - s * 0.5, s, &t, 60);
                            pen.rule(y + h - s * 0.15);
                        }
                        Entry::Note(text) => {
                            let s = size * 0.75;
                            let room = PAGE.0 - 2.0 * MARGIN - number_room;
                            pen.text(MARGIN + number_room, y + h - s * 0.4, s, &measure.fit(text, s, room), 90);
                        }
                        Entry::Song { number, title, length } => {
                            let base = y + h - size * 0.35;
                            let num = number.to_string();
                            pen.text(
                                MARGIN + number_room - size * 0.5 - measure.width(&num, size),
                                base,
                                size,
                                &num,
                                110,
                            );
                            let room = PAGE.0 - 2.0 * MARGIN - number_room - time_room;
                            pen.text(MARGIN + number_room, base, size, &measure.fit(title, size, room), 0);
                            let t = clock(*length);
                            let s = size * 0.8;
                            pen.text(PAGE.0 - MARGIN - measure.width(&t, s), base, s, &t, 60);
                        }
                    }
                    y += h;
                }
                if n + 1 == count {
                    // All songs' time under the last line.
                    pen.rule(y + size * 0.3);
                    let t = clock(total);
                    let s = size * 0.8;
                    pen.text(PAGE.0 - MARGIN - measure.width(&t, s), y + size * 1.3, s, &t, 0);
                }
                surface.finish();
                page.finish();
            }
            writer.finish()
        })
        .map_err(|e| e.to_string())?;
        Ok((bytes, count))
    }
}

#[cfg(target_arch = "wasm32")]
mod pdf {
    use super::Entry;

    pub fn write(_: &str, _: Option<Vec<u8>>, _: &[Entry], _: f64) -> Result<(Vec<u8>, usize), String> {
        Err("printing a setlist needs the desktop program".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryServices;

    fn band() -> Engine {
        let mut e = Engine::new(Box::new(MemoryServices::default()));
        for (title, len) in [("Opener", 200.0), ("Ballad", 241.4), ("Encore", 0.0)] {
            let id = e.run("song.add", &json!({"title": title})).ok().and_then(|v| v["song"].as_u64()).unwrap_or(0);
            if let Some(s) = e.show.song_mut(id) {
                s.duration = len;
            }
        }
        let _ = e.run("playlist.create", &json!({"name": "Town Fair"}));
        let _ = e.run("playlist.add_songs", &json!({"playlist": 0, "set": 0, "songs": [1, 2]}));
        let _ = e.run("playlist.add_set", &json!({"playlist": 0, "name": "Encore"}));
        let _ = e.run("playlist.add_songs", &json!({"playlist": 0, "set": 1, "songs": [3]}));
        e
    }

    #[test]
    fn a_setlist_lists_sets_notes_songs_and_times() {
        let mut e = band();
        assert!(e.run("set.note", &json!({"playlist": 0, "set": 0, "song": 2, "text": " Acoustic guitar "})).is_ok());
        assert!(e.run("set.note", &json!({"playlist": 0, "set": 0, "song": 3, "text": "x"})).is_err(), "not in set");
        assert!(e.run("set.note", &json!({"playlist": 0, "set": 1, "song": 3, "text": "Break"})).is_ok());
        assert!(e.run("set.note", &json!({"playlist": 0, "set": 1, "song": 3, "text": ""})).is_ok(), "taken away");
        let (lines, total) = entries(&e.show, 0).unwrap_or_default();
        assert_eq!(
            lines,
            vec![
                Entry::Set { name: e.show.playlists[0].sets[0].name.clone(), length: 441.4 },
                Entry::Song { number: 1, title: "Opener".into(), length: 200.0 },
                Entry::Note("Acoustic guitar".into()),
                Entry::Song { number: 2, title: "Ballad".into(), length: 241.4 },
                Entry::Set { name: "Encore".into(), length: 0.0 },
                Entry::Song { number: 3, title: "Encore".into(), length: 0.0 },
            ]
        );
        assert_eq!(total, 441.4);
        assert_eq!((clock(241.4), clock(0.0), clock(3725.0)), ("4:01".into(), String::new(), "1:02:05".into()));
    }

    #[test]
    fn a_long_setlist_shrinks_then_takes_more_pages() {
        let songs = |n: usize| -> Vec<Entry> {
            (1..=n).map(|number| Entry::Song { number, title: "Song".into(), length: 180.0 }).collect()
        };
        let (size, pages) = layout::pages(&songs(10), 700.0, 760.0);
        assert_eq!((size, pages.len()), (layout::LARGEST, 1), "a short list is large");
        let (size, pages) = layout::pages(&songs(30), 700.0, 760.0);
        assert!(size < layout::LARGEST && pages.len() == 1, "shrinks to fit one page: {size}");
        let (size, pages) = layout::pages(&songs(80), 700.0, 760.0);
        assert_eq!(size, layout::SMALLEST);
        assert!(pages.len() > 1);
        assert_eq!(pages.iter().map(Vec::len).sum::<usize>(), 80, "every line once");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_setlist_is_written_as_a_pdf() {
        let mut e = band();
        assert!(e.run("playlist.select", &json!({"playlist": 0})).is_ok());
        assert!(e.run("playlist.export_setlist", &json!({"playlist": 0, "path": "/gig/list.txt"})).is_err());
        assert!(e.run("setlist.logo", &json!({"path": "/logo.gif"})).is_err());
        assert!(e.run("setlist.logo", &json!({"path": "/missing.png"})).is_ok());
        let r = e.run("playlist.export_setlist", &json!({"playlist": 0, "path": "/gig/list.pdf"}));
        assert_eq!(r.ok(), Some(json!({"pages": 1, "songs": 3, "length": 441.4})));
        let bytes = e.services().read_file("/gig/list.pdf").unwrap_or_default();
        assert!(bytes.starts_with(b"%PDF-"));
        let _ = e.run("playlist.create", &json!({"name": "Empty"}));
        assert!(e.run("playlist.export_setlist", &json!({"playlist": 1, "path": "/gig/e.pdf"})).is_err());
    }
}
