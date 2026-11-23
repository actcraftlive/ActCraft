//! The window's languages through craft-i18n. English is the source text and the default; every
//! other language is a language file `i18n/<code>.tsv` in this crate (craft-i18n's catalog
//! format: context, English, translation, separated by tabs). Adding a file adds the language:
//! `build.rs` embeds whatever is there, and the file's `@id language.name` row names it in the
//! picker. A missing row shows the English text. See `docs/translating.md`.

use std::cell::Cell;
use std::sync::LazyLock;

use craft_i18n::{I18n, Lang};

// `CATALOGS` and `OFFERED`, one entry per language file.
include!(concat!(env!("OUT_DIR"), "/languages.rs"));

static I18N: LazyLock<I18n> = LazyLock::new(|| I18n::new(CATALOGS, OFFERED).0);

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EN) };
}

/// The setting value for following the system's language.
pub const SYSTEM: &str = "auto";

/// The language choices for Settings: (setting value, name shown). English first, then each
/// language file by code, then the system's language. Names are in their own language.
pub fn choices() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> =
        I18N.languages().into_iter().map(|l| (I18N.code(l).to_owned(), I18N.name(l).to_owned())).collect();
    out.push((SYSTEM.to_owned(), "System language".to_owned()));
    out
}

/// Switches the window's language: `auto` follows the system, a code picks that language file;
/// empty or unknown is English (the default).
pub fn set_language(setting: &str) {
    let lang = match setting {
        SYSTEM => I18N.system_lang(),
        code => I18N.from_code(code).unwrap_or(Lang::EN),
    };
    CURRENT.with(|c| c.set(lang));
}

/// The setting value of the language in use (`en` or a language file's code).
pub fn current_code() -> String {
    I18N.code(CURRENT.with(Cell::get)).to_owned()
}

/// Looks English text up in the current language.
pub fn tr(s: &str) -> &str {
    let lang = CURRENT.with(Cell::get);
    I18N.t(lang, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actcraft_engine::registry;
    use actcraft_show::{MetronomeMode, SongClick};

    /// Every string literal right after one of `calls` in `sources`.
    fn scan(sources: &[&str], calls: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        for src in sources {
            for call in calls {
                let mut rest = *src;
                while let Some(at) = rest.find(call) {
                    let after = rest[at + call.len()..].trim_start();
                    let Some(body) = after.strip_prefix('"') else {
                        rest = after;
                        continue;
                    };
                    let (text, used) = read_literal(body);
                    out.push(text);
                    rest = &body[used..];
                }
            }
        }
        out
    }

    /// The English text of every `tr("…")` call in the window's source, and of every message the
    /// engine and the host show (`say("…")`, `say_with("…", …)` and the error forms).
    fn literals() -> Vec<String> {
        let window = [
            include_str!("lib.rs"),
            include_str!("flows.rs"),
            include_str!("settings.rs"),
            include_str!("lyrics_view.rs"),
            include_str!("regions_view.rs"),
            include_str!("sheet_view.rs"),
            include_str!("notation_view.rs"),
            include_str!("cues_view.rs"),
            include_str!("pads_view.rs"),
        ];
        let engine = [
            include_str!("../../actcraft-engine/src/lib.rs"),
            include_str!("../../actcraft-engine/src/commands.rs"),
            include_str!("../../actcraft-engine/src/region_commands.rs"),
            include_str!("../../actcraft-engine/src/package_commands.rs"),
            include_str!("../../actcraft-engine/src/notation_commands.rs"),
            include_str!("../../actcraft-engine/src/pdf_commands.rs"),
            include_str!("../../actcraft-engine/src/midi_file.rs"),
            include_str!("../../actcraft-engine/src/midi_sync.rs"),
            include_str!("../../actcraft-engine/src/remote.rs"),
            include_str!("../../actcraft-engine/src/macros.rs"),
            include_str!("../../actcraft-engine/src/pads.rs"),
            include_str!("../../actcraft-engine/src/osc.rs"),
            include_str!("../../actcraft-engine/src/voice.rs"),
            include_str!("../../actcraft/src/main.rs"),
        ];
        let mut out = scan(&window, &["tr("]);
        out.extend(scan(&engine, &["say(", "say_error(", "say_with(", "say_error_with("]));
        // Command errors with a fixed text are shown (and translated) without their kind.
        out.extend(scan(&engine, &["invalid(", "failed("]));
        out
    }

    #[test]
    fn engine_messages_show_in_the_window_language() {
        let mut e = actcraft_engine::Engine::new(Box::new(actcraft_engine::services::MemoryServices::default()));
        e.say_with("Opened show with {count} songs", vec![actcraft_engine::MessageArg::plain("count", 3)]);
        e.say_with("Metronome: {mode}", vec![actcraft_engine::MessageArg::word("mode", "Visual only")]);
        let opened = e.messages.get(e.messages.len() - 2).map(crate::message_text);
        let mode = e.messages.back().map(crate::message_text);
        assert_eq!(opened.as_deref(), Some("Opened show with 3 songs"));
        assert_eq!(mode.as_deref(), Some("Metronome: Visual only"));
        set_language("de");
        let opened = e.messages.get(e.messages.len() - 2).map(crate::message_text);
        let mode = e.messages.back().map(crate::message_text);
        set_language("en");
        assert_eq!(opened.as_deref(), Some("Show mit 3 Songs geöffnet"));
        assert_eq!(mode.as_deref(), Some("Metronom: Nur optisch"));
        // The English text stays for the CLI and the control channel.
        assert_eq!(e.messages.back().map(|m| m.text.as_str()), Some("Metronome: Visual only"));
        // A command error shows its message alone, translated; the full text stays English.
        assert!(e.run("region.leave_loop", &serde_json::Value::Null).is_err());
        let error = e.messages.back().cloned();
        assert_eq!(error.as_ref().map(|m| m.text.as_str()), Some("failed: no audio output is running"));
        assert_eq!(error.as_ref().map(crate::message_text).as_deref(), Some("no audio output is running"));
        set_language("de");
        let shown = error.as_ref().map(crate::message_text);
        set_language("en");
        assert_eq!(shown.as_deref(), Some("Kein Audioausgang läuft"));
        // An error that carries a value is translated around it.
        assert!(e.run("song.set", &serde_json::json!({"song": 42, "title": "x"})).is_err());
        let error = e.messages.back().cloned();
        assert_eq!(error.as_ref().map(|m| m.text.as_str()), Some("invalid params: no song with id 42"));
        assert_eq!(error.as_ref().map(crate::message_text).as_deref(), Some("no song with id 42"));
        set_language("de");
        let shown = error.as_ref().map(crate::message_text);
        set_language("en");
        assert_eq!(shown.as_deref(), Some("kein Song mit der ID 42"));
    }

    /// Reads a Rust string literal body up to its closing quote: (text, bytes used).
    fn read_literal(s: &str) -> (String, usize) {
        let mut text = String::new();
        let mut chars = s.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return (text, i + 1),
                '\\' => match chars.next() {
                    Some((_, 'n')) => text.push('\n'),
                    Some((_, 'u')) => {
                        let mut hex = String::new();
                        for (_, h) in chars.by_ref() {
                            match h {
                                '{' => {}
                                '}' => break,
                                h => hex.push(h),
                            }
                        }
                        if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            text.push(ch);
                        }
                    }
                    Some((_, other)) => text.push(other),
                    None => {}
                },
                c => text.push(c),
            }
        }
        (text, s.len())
    }

    /// Text shown through `tr` that isn't a literal at the call.
    fn dynamic() -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in registry().iter() {
            out.push(c.spec.label.to_owned());
            out.extend(c.spec.menu.split('/').filter(|p| !p.is_empty()).map(str::to_owned));
        }
        out.extend(MetronomeMode::ALL.iter().map(|m| m.label().to_owned()));
        out.extend(SongClick::ALL.iter().map(|m| m.label().to_owned()));
        out.extend(actcraft_show::LyricsScroll::ALL.iter().map(|m| m.label().to_owned()));
        for f in actcraft_library::smart::FIELDS {
            out.push(crate::flows::field_label(f).to_owned());
        }
        for o in actcraft_library::smart::OPS {
            out.push(crate::flows::op_label(o).to_owned());
        }
        out.extend(crate::sheet_view::MARK_COLOURS.iter().map(|c| c.0.to_owned()));
        // Command errors with values are shown through these templates.
        out.extend(actcraft_engine::error_text::TEMPLATES.iter().map(|t| (*t).to_owned()));
        for m in [
            actcraft_show::RegionMode::Play,
            actcraft_show::RegionMode::Loop,
            actcraft_show::RegionMode::Skip,
            actcraft_show::RegionMode::StopAfter,
        ] {
            out.push(crate::regions_view::mode_label(m).to_owned());
        }
        for clock in [
            crate::ClockMode::Elapsed,
            crate::ClockMode::Remaining,
            crate::ClockMode::QueueEnd,
            crate::ClockMode::TimeOfDay,
            crate::ClockMode::ShowTime,
        ] {
            out.push(clock.label().to_owned());
        }
        for s in [
            "Name",
            "Start",
            "End",
            "What happens",
            "Cue",
            "Spoken",
            "Part",
            "Starts at",
            "Intro",
            "Verse",
            "Chorus",
            "Bridge",
            "Solo",
            "Interlude",
            "Outro",
            "End 3 2 1",
            "red",
            "orange",
            "yellow",
            "green",
            "blue",
            "purple",
            "pink",
            "grey",
            "sans",
            "serif",
            "mono",
            "script",
        ] {
            out.push(s.to_owned());
        }
        out.push("System language".to_owned());
        // Messages whose template is chosen by an `if`.
        for s in ["{group} muted", "{group} unmuted", "Metronome on", "Metronome off"] {
            out.push(s.to_owned());
        }
        out
    }

    /// Every language file in `i18n/`.
    fn files() -> Vec<Lang> {
        I18N.languages().into_iter().filter(|&l| l != Lang::EN).collect()
    }

    #[test]
    fn language_files_parse_cleanly() {
        let problems = I18N.check_all();
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn every_language_file_is_offered_and_named() {
        assert_eq!(files().len(), OFFERED.len(), "each i18n/<code>.tsv is a language");
        for lang in files() {
            assert_ne!(
                I18N.name(lang),
                I18N.code(lang),
                "i18n/{}.tsv needs an `@id language.name` row",
                I18N.code(lang)
            );
        }
        let codes: Vec<String> = choices().into_iter().map(|(c, _)| c).collect();
        assert_eq!(codes.first().map(String::as_str), Some("en"), "English comes first");
        assert!(codes.iter().any(|c| c == "de"));
        assert_eq!(codes.last().map(String::as_str), Some(SYSTEM));
    }

    #[test]
    fn every_window_text_is_in_every_language_file() {
        let texts: Vec<String> =
            literals().into_iter().chain(dynamic()).filter(|s| !s.trim().is_empty() && s != "?").collect();
        let mut report = Vec::new();
        for lang in files() {
            // Names that read the same in both languages are listed too, so a missing row is a gap.
            let mut missing: Vec<&str> =
                texts.iter().map(String::as_str).filter(|s| I18N.get(lang, "", s).is_none()).collect();
            missing.sort_unstable();
            missing.dedup();
            if !missing.is_empty() {
                report.push(format!(
                    "i18n/{}.tsv lacks {} rows:\n{}",
                    I18N.code(lang),
                    missing.len(),
                    missing.join("\n")
                ));
            }
        }
        assert!(report.is_empty(), "{}", report.join("\n\n"));
    }

    #[test]
    fn english_is_the_default_and_switching_changes_the_text() {
        assert_eq!(current_code(), "en");
        assert_eq!(tr("Play"), "Play");
        set_language("de");
        assert_eq!(current_code(), "de");
        assert_eq!(tr("Play"), "Abspielen");
        set_language("");
        assert_eq!(tr("Play"), "Play");
        set_language("xx");
        assert_eq!(tr("Play"), "Play");
        assert_eq!(current_code(), "en");
    }
}
