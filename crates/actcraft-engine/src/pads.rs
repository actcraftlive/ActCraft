//! Pads: buttons on the screen that run a command or a macro with one press, for what a band
//! needs at hand during a show (the next song, the click, a light change).

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_PAD_PAGES, MAX_PADS};
use actcraft_show::{FaderTarget, Macro, Pad, ToggleTarget, parse_colour};
use serde_json::{Value, json};

use crate::Engine;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// `pad.add {label, colour?, command? | macro? | fader?}`: a pad that runs a command that needs
/// nothing more or a macro, or a fader that moves a volume (`master`, `song`, or `bus` with
/// `bus: index`).
pub fn pad_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let label = p::str_max(prm, "label", 64)?.trim().to_owned();
    if label.is_empty() {
        return Err(invalid("a pad needs a label"));
    }
    let command = p::opt_str_max(prm, "command", 64)?.unwrap_or_default().to_owned();
    let macro_name = p::opt_str_max(prm, "macro", 32)?.unwrap_or_default().trim().to_ascii_uppercase();
    let fader = match p::opt_str_max(prm, "fader", 16)? {
        None => None,
        Some("master") => Some(FaderTarget::Master),
        Some("song") => Some(FaderTarget::Song),
        Some("bus") => {
            let bus = p::i64_in(prm, "bus", 0, actcraft_show::limits::BUSES as i64 - 1)?;
            Some(FaderTarget::Bus(u8::try_from(bus).unwrap_or(0)))
        }
        Some(_) => return Err(invalid("`fader` must be master, song or bus")),
    };
    let toggle = match p::opt_str_max(prm, "toggle", 16)? {
        None => None,
        Some("metronome") => Some(ToggleTarget::Metronome),
        Some("loop_song") => Some(ToggleTarget::LoopSong),
        Some("autoplay") => Some(ToggleTarget::Autoplay),
        Some("blackout") => Some(ToggleTarget::Blackout),
        Some("freeze") => Some(ToggleTarget::Freeze),
        Some("blind") => Some(ToggleTarget::Blind),
        Some(_) => return Err(invalid("`toggle` must be metronome, loop_song, autoplay, blackout, freeze or blind")),
    };
    let kinds = [!command.is_empty(), !macro_name.is_empty(), fader.is_some(), toggle.is_some()];
    if kinds.iter().filter(|k| **k).count() != 1 {
        return Err(invalid("a pad runs a command or a macro, moves a volume or switches something: one of them"));
    }
    if (!command.is_empty() && !crate::remote::runnable(&command))
        || (!macro_name.is_empty() && !Macro::valid_name(&macro_name))
    {
        return Err(invalid("a pad runs a command that needs nothing more, or a macro"));
    }
    let page = page_param(prm, "page")?.unwrap_or(e.pad_page);
    let mut pad = Pad { label, command, macro_name, fader, toggle, page: page_u8(page), ..Pad::default() };
    if let Some(c) = p::opt_str_max(prm, "colour", 16)? {
        if parse_colour(c).is_none() {
            return Err(invalid("`colour` must look like #rrggbb"));
        }
        c.clone_into(&mut pad.colour);
    }
    if e.show.pads.len() >= MAX_PADS {
        return Err(failed("a show holds at most 64 pads"));
    }
    e.show.pads.push(pad);
    e.mark_changed();
    Ok(json!({"pad": e.show.pads.len() - 1}))
}

/// Whether what a toggle pad switches is on now.
pub fn toggle_state(e: &Engine, t: ToggleTarget) -> bool {
    match t {
        ToggleTarget::Metronome => e.show.metronome.enabled,
        ToggleTarget::LoopSong => e.current().is_some_and(|s| s.loop_song),
        ToggleTarget::Autoplay => e.current().is_some_and(|s| s.autoplay),
        ToggleTarget::Blackout => e.dmx.blackout,
        ToggleTarget::Freeze => e.frozen(),
        ToggleTarget::Blind => e.dmx.blind(),
    }
}

/// `pad.remove {pad}`.
pub fn pad_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "pad", e.show.pads.len())?;
    e.show.pads.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `pad.list`: every pad.
pub fn pad_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show
            .pads
            .iter()
            .map(|p| {
                json!({"label": p.label, "colour": p.colour, "command": p.command, "macro": p.macro_name, "fader": p.fader, "toggle": p.toggle, "page": p.page})
            })
            .collect(),
    ))
}

fn page_param(prm: &Value, key: &str) -> Result<Option<usize>, CommandError> {
    Ok(p::opt_i64_in(prm, key, 0, MAX_PAD_PAGES as i64 - 1)?.and_then(|n| usize::try_from(n).ok()))
}

fn page_u8(page: usize) -> u8 {
    u8::try_from(page.min(MAX_PAD_PAGES - 1)).unwrap_or(0)
}

impl Engine {
    /// The pad page the bar shows (from 0).
    pub fn pad_page(&self) -> usize {
        self.pad_page
    }

    /// How many pad pages there are: up to the last one holding a pad, or the one shown.
    pub fn pad_pages(&self) -> usize {
        let used = self.show.pads.iter().map(|p| usize::from(p.page) + 1).max().unwrap_or(1);
        used.max(self.pad_page + 1)
    }

    /// The pads on the page shown, as indexes into every pad, in order.
    pub fn shown_pads(&self) -> Vec<usize> {
        let page = page_u8(self.pad_page);
        self.show.pads.iter().enumerate().filter(|(_, p)| p.page == page).map(|(i, _)| i).collect()
    }
}

/// `pad.page {page}`: the bar shows this pad page (from 0); a page past the last starts a new one.
pub fn pad_page(e: &mut Engine, prm: &Value) -> CommandResult {
    let page = page_param(prm, "page")?.ok_or_else(|| invalid("give `page`"))?;
    if page > e.pad_pages() {
        return Err(invalid("that page is past the next new one"));
    }
    e.pad_page = page;
    Ok(json!({"page": page, "pages": e.pad_pages()}))
}

/// `pad.page_next`: the next pad page, from the last back to the first.
pub fn pad_page_next(e: &mut Engine, _prm: &Value) -> CommandResult {
    e.pad_page = (e.pad_page + 1) % e.pad_pages();
    Ok(json!({"page": e.pad_page, "pages": e.pad_pages()}))
}

/// `pad.page_before`: the pad page before, from the first round to the last.
pub fn pad_page_before(e: &mut Engine, _prm: &Value) -> CommandResult {
    let pages = e.pad_pages();
    e.pad_page = (e.pad_page + pages - 1) % pages;
    Ok(json!({"page": e.pad_page, "pages": e.pad_pages()}))
}

/// `pad.move {pad, page}`: puts a pad on another page (at the end of it).
pub fn pad_move(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "pad", e.show.pads.len())?;
    let page = page_param(prm, "page")?.ok_or_else(|| invalid("give `page`"))?;
    let mut pad = e.show.pads.remove(i);
    pad.page = page_u8(page);
    e.show.pads.push(pad);
    e.mark_changed();
    Ok(json!({"pad": e.show.pads.len() - 1}))
}

/// `pad.press {pad}`: runs what the pad runs.
pub fn pad_press(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "pad", e.show.pads.len())?;
    let Some(pad) = e.show.pads.get(i).cloned() else { return Err(invalid("no such pad")) };
    if pad.fader.is_some() {
        return Err(invalid("a fader is moved, not pressed"));
    }
    if let Some(t) = pad.toggle {
        let on = toggle_state(e, t);
        match t {
            ToggleTarget::Metronome => {
                e.run("metronome.toggle", &Value::Null)?;
            }
            ToggleTarget::Blackout => {
                e.run("dmx.blackout", &Value::Null)?;
            }
            ToggleTarget::Freeze => {
                e.run("lighting.freeze", &Value::Null)?;
            }
            ToggleTarget::Blind => {
                e.run("lighting.blind", &Value::Null)?;
            }
            ToggleTarget::LoopSong | ToggleTarget::Autoplay => {
                let id = e.current_song.ok_or_else(|| invalid("no song chosen"))?;
                let field = if t == ToggleTarget::LoopSong { "loop_song" } else { "autoplay" };
                e.run("song.set", &json!({"song": id, field: !on}))?;
            }
        }
        return Ok(json!({"on": toggle_state(e, t)}));
    }
    if !pad.macro_name.is_empty() {
        let ran = e.run_macro(&pad.macro_name)?;
        return Ok(json!({"ran": ran}));
    }
    if !crate::remote::runnable(&pad.command) {
        return Err(failed("this pad's command can't run without more"));
    }
    e.run(&pad.command, &Value::Null)?;
    Ok(json!({"ran": 1}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_switch_and_show_their_state() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("song.add", &Value::Null);
        assert!(e.run("pad.add", &json!({"label": "Click", "toggle": "metronome"})).is_ok());
        assert!(e.run("pad.add", &json!({"label": "Loop", "toggle": "loop_song"})).is_ok());
        assert!(e.run("pad.add", &json!({"label": "x", "toggle": "lights"})).is_err());
        assert!(e.run("pad.add", &json!({"label": "x", "toggle": "autoplay", "command": "transport.stop"})).is_err());
        let click = toggle_state(&e, ToggleTarget::Metronome);
        assert_eq!(e.run("pad.press", &json!({"pad": 0})).ok(), Some(json!({"on": !click})));
        assert_eq!(toggle_state(&e, ToggleTarget::Metronome), !click);
        assert!(!toggle_state(&e, ToggleTarget::LoopSong));
        assert_eq!(e.run("pad.press", &json!({"pad": 1})).ok(), Some(json!({"on": true})));
        assert!(e.current().is_some_and(|s| s.loop_song), "the song loops now");
        assert_eq!(e.run("pad.press", &json!({"pad": 1})).ok(), Some(json!({"on": false})));
    }

    #[test]
    fn pads_are_added_pressed_and_removed() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("song.add", &Value::Null);
        let _ = e.run("song.add", &Value::Null);
        let first = e.current_song;
        let r = e.run("pad.add", &json!({"label": "Next", "command": "transport.next_song", "colour": "#3dc97a"}));
        assert_eq!(r.ok(), Some(json!({"pad": 0})));
        assert!(e.run("macro.set", &json!({"name": "BACK", "commands": ["transport.previous_song"]})).is_ok());
        assert!(e.run("pad.add", &json!({"label": "Back", "macro": "back"})).is_ok());
        for bad in [
            json!({"label": "", "command": "transport.stop"}),
            json!({"label": "x", "command": "show.open"}),
            json!({"label": "x", "command": "transport.stop", "macro": "A"}),
            json!({"label": "x", "command": "transport.stop", "fader": "master"}),
            json!({"label": "x", "fader": "bus", "bus": 99}),
            json!({"label": "x", "fader": "lights"}),
            json!({"label": "x"}),
            json!({"label": "x", "macro": "2BAD"}),
            json!({"label": "x", "command": "transport.stop", "colour": "green"}),
        ] {
            assert!(e.run("pad.add", &bad).is_err(), "{bad}");
        }
        assert_eq!(e.run("pad.press", &json!({"pad": 0})).ok(), Some(json!({"ran": 1})));
        assert_ne!(e.current_song, first);
        assert_eq!(e.run("pad.press", &json!({"pad": 1})).ok(), Some(json!({"ran": 1})), "the macro");
        assert_eq!(e.current_song, first);
        assert!(e.run("pad.press", &json!({"pad": 9})).is_err());
        let r = e.run("pad.add", &json!({"label": "Monitors", "fader": "bus", "bus": 2}));
        assert_eq!(r.ok(), Some(json!({"pad": 2})));
        assert_eq!(e.show.pads[2].fader, Some(FaderTarget::Bus(2)));
        assert!(e.run("pad.press", &json!({"pad": 2})).is_err(), "a fader is moved, not pressed");
        assert_eq!(e.run("pad.list", &Value::Null).ok().map(|v| v[1]["macro"].clone()), Some(json!("BACK")));
        assert!(e.run("pad.remove", &json!({"pad": 0})).is_ok());
        assert_eq!(e.show.pads.len(), 2);
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.pads).unwrap_or_default();
        assert_eq!(back, e.show.pads);
    }

    #[test]
    fn pads_sit_on_pages_that_are_shown_one_at_a_time() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let add = |e: &mut Engine, label: &str, page: Option<u8>| {
            let mut prm = json!({"label": label, "command": "metronome.toggle"});
            if let Some(p) = page {
                prm["page"] = json!(p);
            }
            e.run("pad.add", &prm)
        };
        assert!(add(&mut e, "A", None).is_ok());
        assert!(add(&mut e, "B", Some(1)).is_ok());
        assert!(add(&mut e, "C", None).is_ok());
        assert_eq!((e.pad_page(), e.pad_pages()), (0, 2));
        assert_eq!(e.shown_pads(), vec![0, 2]);
        assert_eq!(e.run("pad.page_next", &Value::Null).ok(), Some(json!({"page": 1, "pages": 2})));
        assert_eq!(e.shown_pads(), vec![1]);
        // OSC pad numbers count on the page shown.
        assert_eq!(e.osc_in("/actcraft/pad/1", Some(1.0)), 1);
        assert_eq!(e.osc_in("/actcraft/pad/2", Some(1.0)), 0, "page 2 holds one pad");
        assert_eq!(
            e.run("pad.page_next", &Value::Null).ok(),
            Some(json!({"page": 0, "pages": 2})),
            "round to the first"
        );
        assert_eq!(e.run("pad.page_before", &Value::Null).ok(), Some(json!({"page": 1, "pages": 2})));
        // A new, empty page, and new pads land on the page shown.
        assert_eq!(e.run("pad.page", &json!({"page": 2})).ok(), Some(json!({"page": 2, "pages": 3})));
        assert!(e.run("pad.page", &json!({"page": 4})).is_err(), "past the next new page");
        assert!(e.run("pad.page", &json!({"page": 8})).is_err());
        assert!(add(&mut e, "D", None).is_ok());
        assert_eq!(e.show.pads[3].page, 2);
        // Moving a pad puts it at the end of its new page.
        assert_eq!(e.run("pad.move", &json!({"pad": 0, "page": 2})).ok(), Some(json!({"pad": 3})));
        assert_eq!(e.shown_pads(), vec![2, 3]);
        assert!(e.run("pad.move", &json!({"pad": 0, "page": 9})).is_err());
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.pads).unwrap_or_default();
        assert_eq!(back, e.show.pads);
        // A file from elsewhere can't put a pad past the last page.
        let file = br#"{"format":"actcraft-show","version":1,"name":"P","pads":[{"label":"X","command":"transport.toggle","page":200}]}"#;
        assert_eq!(actcraft_show::Show::from_bytes(file).map(|s| s.pads[0].page).ok(), Some(7));
    }
}
