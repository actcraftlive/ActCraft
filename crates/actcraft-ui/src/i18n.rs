//! The window's languages: English (the source text) and German, through craft-i18n.
//!
//! The German catalog is `i18n/de.tsv` in this crate (craft-i18n's catalog format: context, English,
//! translation, separated by tabs). A missing row shows the English text.

use std::cell::Cell;
use std::sync::LazyLock;

use craft_i18n::{Embedded, I18n, Lang};

/// (locale, shared catalog, ActCraft's catalog).
static CATALOGS: &Embedded = &[("de", "", include_str!("../i18n/de.tsv"))];

/// The languages offered besides English.
const OFFERED: &[&str] = &["de"];

static I18N: LazyLock<I18n> = LazyLock::new(|| I18n::new(CATALOGS, OFFERED).0);

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EN) };
}

/// The language choices for Settings: (setting value, name shown).
pub fn choices() -> Vec<(&'static str, &'static str)> {
    vec![("auto", "System language"), ("en", "English"), ("de", "Deutsch")]
}

/// Switches the window's language: `auto` follows the system, `en` or `de` pick one; anything
/// else is English.
pub fn set_language(setting: &str) {
    let lang = match setting {
        "auto" | "" => I18N.system_lang(),
        code => I18N.from_code(code).unwrap_or(Lang::EN),
    };
    CURRENT.with(|c| c.set(lang));
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

    /// The English text of every `tr("…")` call in the window's source.
    fn literals() -> Vec<String> {
        let sources = [
            include_str!("lib.rs"),
            include_str!("flows.rs"),
            include_str!("settings.rs"),
            include_str!("lyrics_view.rs"),
            include_str!("regions_view.rs"),
        ];
        let mut out = Vec::new();
        for src in sources {
            let mut rest = src;
            while let Some(at) = rest.find("tr(\"") {
                let after = &rest[at + 4..];
                let (text, used) = read_literal(after);
                out.push(text);
                rest = &after[used..];
            }
        }
        out
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
        for f in actcraft_library::smart::FIELDS {
            out.push(crate::flows::field_label(f).to_owned());
        }
        for o in actcraft_library::smart::OPS {
            out.push(crate::flows::op_label(o).to_owned());
        }
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
        ] {
            out.push(clock.label().to_owned());
        }
        for s in [
            "Name",
            "Start",
            "End",
            "What happens",
            "Cue",
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
        out.extend(choices().iter().map(|(_, n)| (*n).to_owned()));
        out
    }

    #[test]
    fn german_catalog_parses_cleanly() {
        let problems = I18N.check_all();
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn every_window_text_has_a_german_translation() {
        set_language("de");
        let mut missing: Vec<String> = literals()
            .into_iter()
            .chain(dynamic())
            .filter(|s| !s.trim().is_empty() && s != "?")
            // Names that read the same in both languages are listed in the catalog too, so a
            // missing row is always a gap.
            .filter(|s| I18N.from_code("de").is_some_and(|de| I18N.get(de, "", s).is_none()))
            .collect();
        missing.sort();
        missing.dedup();
        set_language("en");
        assert!(missing.is_empty(), "{} texts have no German row:\n{}", missing.len(), missing.join("\n"));
    }

    #[test]
    fn switching_language_changes_the_text() {
        set_language("de");
        assert_eq!(tr("Play"), "Abspielen");
        set_language("en");
        assert_eq!(tr("Play"), "Play");
        set_language("xx");
        assert_eq!(tr("Play"), "Play");
    }
}
