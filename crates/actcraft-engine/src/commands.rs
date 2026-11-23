//! Every engine command. Params are JSON objects; each command checks its own params and returns
//! a JSON result. Ids follow `area.action`.

use actcraft_audio::Msg;
use actcraft_command::params as p;
use actcraft_command::{Command, CommandError, CommandResult, CommandSpec, Registry, always};
use actcraft_library::filename;
use actcraft_library::history;
use actcraft_library::order::{self, SortKey};
use actcraft_library::queue::DoubleTap;
use actcraft_library::search::{self, Query};
use actcraft_library::smart;
use actcraft_show::limits::{
    BUSES, MAX_PLAYLISTS, MAX_SET_SONGS, MAX_SETS, MAX_SONGS, MAX_TEXT, MAX_TRACKS, MUTE_GROUPS,
};
use actcraft_show::{MetronomeMode, Playlist, Set, Show, SmartRule, SmartRules, Song, Track, parse_colour};
use serde_json::{Value, json};

use crate::{Engine, registry, services};

type Run = fn(&mut Engine, &Value) -> CommandResult;

fn cmd(
    id: &'static str,
    label: &'static str,
    menu: &'static str,
    shortcut: Option<&'static str>,
    params: &'static str,
    run: Run,
) -> Command<Engine> {
    Command { spec: CommandSpec { id, label, menu, shortcut, params }, enabled: always, run }
}

/// Enabled when a song is chosen for playback (transport commands act on it).
fn with_song(mut c: Command<Engine>) -> Command<Engine> {
    c.enabled = |e: &Engine| e.current_song.is_some_and(|id| e.show.song(id).is_some());
    c
}

/// Enabled when the show has songs; the command takes a `song` param or uses the current song.
fn with_songs(mut c: Command<Engine>) -> Command<Engine> {
    c.enabled = |e: &Engine| !e.show.songs.is_empty();
    c
}

/// Enabled while a song is chosen (the command works on it unless given another).
fn with_current_song(mut c: Command<Engine>) -> Command<Engine> {
    c.enabled = |e: &Engine| e.current_song.is_some_and(|id| e.show.song(id).is_some());
    c
}

/// Enabled while a playlist is open (the command works on it unless given another).
fn with_open_playlist(mut c: Command<Engine>) -> Command<Engine> {
    c.enabled = |e: &Engine| e.current_playlist.is_some_and(|i| i < e.show.playlists.len());
    c
}

/// Every command, in menu order.
pub fn all() -> Vec<Command<Engine>> {
    vec![
        // App
        cmd("app.about", "About ActCraft", "Help/About ActCraft", None, "none", app_about),
        cmd("app.commands", "List commands", "Help/List Commands", None, "none", app_commands),
        cmd("app.messages", "Recent messages", "", None, "none", app_messages),
        // Show file
        cmd(
            "show.new",
            "New show",
            "File/New Show",
            Some("Ctrl+N"),
            "{discard?: bool} (needed when there are unsaved changes)",
            show_new,
        ),
        cmd("show.open", "Open show", "File/Open Show…", Some("Ctrl+O"), "{path: text, discard?: bool}", show_open),
        cmd(
            "show.save",
            "Save show",
            "File/Save Show",
            Some("Ctrl+S"),
            "{path?: text} (defaults to where it was opened)",
            show_save,
        ),
        cmd("show.save_as", "Save show as", "File/Save Show As…", Some("Ctrl+Shift+S"), "{path: text}", show_save_as),
        cmd(
            "show.export_package",
            "Export show package",
            "File/Export Show Package…",
            None,
            "{path: text, song?: id, playlist?: index} (.actcraftpack: the show, or only that song or that playlist and its songs, with every audio, MIDI, PDF and score file they use)",
            crate::package_commands::show_export_package,
        ),
        cmd(
            "show.open_package",
            "Open show package",
            "File/Open Show Package…",
            None,
            "{path: text, folder?: text (where the files go; default: a folder beside the package), discard?: bool}",
            crate::package_commands::show_open_package,
        ),
        cmd("show.info", "Show details", "", None, "none", show_info),
        cmd("show.rename", "Rename show", "File/Rename Show…", None, "{name: text}", show_rename),
        // Songs
        cmd("song.list", "List songs", "", None, "none", song_list),
        cmd(
            "song.add",
            "New song",
            "Song/New Song",
            None,
            "{title?, artist?, key?, bpm?, …any song.set field}",
            song_add,
        ),
        cmd(
            "song.set",
            "Edit song details",
            "",
            None,
            "{song?: id, title?, artist?, key?, bpm?, beats_per_bar?, year?, keywords?: [text], capo?, transpose?, start?, end?: number|null, count_in_bars?, click?: off|whole_song|count_in_only|visual_only, beat_anchor?, volume_db?, autoplay?, loop_song?, announce_title?: bool, fade_out?, midi_file?, pdf_file?: text|null, notation_file?: text|null (.musicxml, .xml, .mxl), score_first_bar?: song bar of the score's bar 1 (-999..9999, 0 or less for a pickup), lyrics?, lyrics_scroll?: line|smooth|constant|off}",
            song_set,
        ),
        cmd("song.remove", "Delete song", "Song/Delete Song…", None, "{song: id, confirm: true}", song_remove),
        cmd("song.load", "Load song", "", None, "{song: id}", song_load),
        cmd(
            "song.import_files",
            "Add songs from audio files",
            "Song/Add Songs from Audio Files…",
            None,
            "{paths: [text], format?: text like \"%artist% - %title%\"}",
            song_import_files,
        ),
        cmd("song.duplicates", "Find duplicate songs", "Song/Find Duplicates", None, "none", song_duplicates),
        with_current_song(cmd(
            "song.export_package",
            "Export song package",
            "Song/Export Song Package…",
            None,
            "{path: text, song?: id (the current song when left out)} (.actcraftpack: the song with its files)",
            crate::package_commands::song_export_package,
        )),
        with_songs(cmd(
            "region.add",
            "Add region",
            "Song/Add Region at Playhead",
            Some("R"),
            "{song?: id, name?, start?: seconds (the playhead when left out), end?, mode?: play|loop|skip|stop_after, colour?, cue?, voice?: words spoken before it}",
            crate::region_commands::region_add,
        )),
        with_songs(cmd(
            "region.set",
            "Edit region",
            "",
            None,
            "{song?: id, region: index, name?, start?, end?, mode?, colour?, cue?, voice?}",
            crate::region_commands::region_set,
        )),
        with_songs(cmd(
            "region.remove",
            "Remove region",
            "",
            None,
            "{song?: id, region: index}",
            crate::region_commands::region_remove,
        )),
        with_songs(cmd(
            "region.jump",
            "Jump to region",
            "",
            None,
            "{song?: id, region: index}",
            crate::region_commands::region_jump,
        )),
        cmd(
            "region.leave_loop",
            "Leave loop",
            "Transport/Leave Loop",
            Some("L"),
            "none",
            crate::region_commands::region_leave_loop,
        ),
        with_songs(cmd(
            "region.import_markers",
            "Regions from file markers",
            "Song/Regions from File Markers",
            None,
            "{song?: id, track?: index}",
            crate::region_commands::region_import_markers,
        )),
        with_songs(cmd(
            "lyrics.import",
            "Import lyrics",
            "Song/Import Lyrics…",
            None,
            "{song?: id, path: text} (.txt, .cho/.chopro/.chordpro/.crd/.pro, .lrc, .docx: the document's text)",
            lyrics_import,
        )),
        with_songs(cmd(
            "notation.attach",
            "Attach notation",
            "Song/Attach Notation…",
            None,
            "{song?: id, path: text} (MusicXML: .musicxml, .xml, .mxl)",
            crate::notation_commands::notation_attach,
        )),
        with_songs(cmd(
            "pdf.mark",
            "Mark the PDF",
            "",
            None,
            "{song?: id, page: 0-based, kind?: pen|highlight|note, points: [[x, y]] (fractions of the page from the top left; pen: the stroke, highlight: two corners, note: one point), colour?: #rrggbb, text?: the note's words} -> {mark: index}",
            crate::pdf_commands::pdf_mark,
        )),
        with_songs(cmd(
            "pdf.unmark",
            "Remove a PDF mark",
            "",
            None,
            "{song?: id, mark: index}",
            crate::pdf_commands::pdf_unmark,
        )),
        with_songs(cmd(
            "pdf.clear_marks",
            "Remove PDF marks",
            "",
            None,
            "{song?: id, page?: 0-based (all pages when left out), confirm: true} -> {removed}",
            crate::pdf_commands::pdf_clear_marks,
        )),
        with_songs(cmd(
            "notation.read",
            "Read notation",
            "",
            None,
            "{song?: id} -> {title, composer, tempo, parts: [{name, bars, played_bars (repeats and jumps unrolled), notes, tablature, strings, capo}]}",
            crate::notation_commands::notation_read,
        )),
        with_songs(cmd(
            "midi.attach",
            "Attach MIDI file",
            "",
            None,
            "{song?: id, path: text} (.mid, .midi, .smf: played to the MIDI output with the song, from its start) -> {tracks, names, notes, seconds}",
            crate::midi_file::midi_attach,
        )),
        with_songs(cmd(
            "midi.read",
            "Read MIDI file",
            "",
            None,
            "{song?: id} -> {tracks, names, notes, seconds}",
            crate::midi_file::midi_read,
        )),
        with_songs(cmd(
            "lyrics.stamp_line",
            "Set line timecode",
            "",
            None,
            "{song?: id, line: index, seconds?: number (default: the playback time)}",
            lyrics_stamp_line,
        )),
        with_songs(cmd(
            "lyrics.replace",
            "Replace in lyrics",
            "",
            None,
            "{song?: id, find: text, replace?: text, match_case?: bool}",
            lyrics_replace,
        )),
        with_songs(cmd(
            "song.detect_bpm",
            "Detect tempo and first beat",
            "Song/Detect Tempo and First Beat",
            None,
            "{song?: id}",
            song_detect_bpm,
        )),
        cmd(
            "library.set_filename_format",
            "Song details from file names",
            "",
            None,
            "{format: text like \"%artist% - %title%\"}",
            library_set_filename_format,
        ),
        // Tracks
        with_songs(cmd(
            "track.add",
            "Add track",
            "Song/Add Tracks…",
            None,
            "{song?: id, name?: text, file?: text}",
            track_add,
        )),
        with_songs(cmd("track.remove", "Remove track", "", None, "{song?: id, track: index}", track_remove)),
        with_songs(cmd(
            "track.set",
            "Edit track",
            "",
            None,
            "{song?: id, track: index, name?, colour?: \"#rrggbb\", file?, volume_db?, pan?: -1..1, mute?, bus?: 0..15, mono_to_stereo?}",
            track_set,
        )),
        with_songs(cmd(
            "track.set_eq",
            "Set track EQ",
            "",
            None,
            "{song?: id, track: index, band: 0..3, freq?, gain_db?: -24..24, q?}",
            track_set_eq,
        )),
        with_songs(cmd(
            "track.mute_group",
            "Put track in mute group",
            "",
            None,
            "{song?: id, track: index, group: 1..6, member: bool}",
            track_mute_group,
        )),
        // Mixer
        cmd(
            "bus.set",
            "Edit output bus",
            "",
            None,
            "{bus: 0..15, name?, volume_db?, mute?, output?: first device channel}",
            bus_set,
        ),
        cmd("mixer.master_volume", "Master volume", "", None, "{db: -96..12}", mixer_master),
        with_songs(cmd("mixer.song_volume", "Song volume", "", None, "{song?: id, db: -96..12}", mixer_song)),
        cmd(
            "mute_group.toggle",
            "Toggle mute group",
            // No menu item: the mixer has one button per group.
            "",
            None,
            "{group: 1..6}",
            mute_group_toggle,
        ),
        cmd(
            "mute_group.set",
            "Edit mute group",
            "",
            None,
            "{group: 1..6, active?: bool, label?: text}",
            mute_group_set,
        ),
        // Transport
        with_song(cmd(
            "transport.play",
            "Play",
            "Transport/Play",
            None,
            "{count_in?: bool (default true), from?: seconds (start there; the song must be loaded)}",
            transport_play,
        )),
        cmd("transport.pause", "Pause", "Transport/Pause", None, "none", transport_pause),
        cmd("transport.stop", "Stop", "Transport/Stop (back to the start)", Some("Escape"), "none", transport_stop),
        with_song(cmd(
            "transport.toggle",
            "Play or pause",
            "Transport/Play or Pause",
            Some("Space"),
            "none",
            transport_toggle,
        )),
        with_song(cmd("transport.seek", "Go to time", "", None, "{seconds: number}", transport_seek)),
        cmd("transport.next_song", "Next song", "Transport/Next Song", Some("Ctrl+Right"), "none", transport_next),
        cmd(
            "transport.previous_song",
            "Previous song",
            "Transport/Previous Song",
            Some("Ctrl+Left"),
            "none",
            transport_previous,
        ),
        cmd(
            "transport.crossfade_to",
            "Crossfade to song",
            "",
            None,
            "{song: id, seconds?: number}",
            transport_crossfade_to,
        ),
        cmd("transport.set_crossfade", "Set crossfade length", "", None, "{seconds: 0..30}", transport_set_crossfade),
        cmd(
            "transport.set_play_guard",
            "Set double-trigger protection",
            "",
            None,
            "{ms: 0..5000}",
            transport_set_guard,
        ),
        // Metronome
        cmd("metronome.toggle", "Metronome on or off", "Metronome/On or Off", Some("M"), "none", metronome_toggle),
        cmd("metronome.cycle_mode", "Next metronome mode", "Metronome/Next Mode", None, "none", metronome_cycle),
        cmd(
            "metronome.set",
            "Metronome settings",
            "",
            None,
            "{enabled?, mode?: audible|count_in_only|visual_only, volume_db?, bus?, quantized_start?, latency_offset_ms?, run_while_stopped?}",
            metronome_set,
        ),
        cmd(
            "voice.set",
            "Spoken cue settings",
            "",
            None,
            "{enabled?: bool, bus?: index, volume_db?: number, lead_beats?: 0-32, language?: code (empty: the computer's own)}",
            crate::voice::voice_set,
        ),
        cmd(
            "midi.set",
            "MIDI time settings",
            "",
            None,
            "{clock?: bool (MIDI Clock with start, stop and song position), timecode?: bool (MIDI Timecode), timecode_fps?: 24|25|30, latency_offset_ms?: -500..500 (moves the song's MIDI file earlier or later)} -> the settings",
            crate::midi_sync::midi_set,
        ),
        cmd(
            "remote.add",
            "Add remote action",
            "",
            None,
            "{kind: note|cc|program, channel?: 1-16 (any when left out), number: 0-127, command: id (one that needs nothing more)} -> {action: index}",
            crate::remote::remote_add,
        ),
        cmd("remote.remove", "Remove remote action", "", None, "{action: index}", crate::remote::remote_remove),
        cmd("remote.list", "List remote actions", "", None, "none", crate::remote::remote_list),
        cmd(
            "remote.trigger",
            "Set off remote actions",
            "",
            None,
            "{kind: note|cc|program, channel?: 1-16, number: 0-127, value?: 0-127} (as if the MIDI message came in) -> {ran}",
            crate::remote::remote_trigger,
        ),
        cmd(
            "osc.trigger",
            "Act on an OSC message",
            "",
            None,
            "{address: /actcraft/<command id> | /actcraft/macro/<NAME> | /actcraft/pad/<n>, value?: number (0 or less does nothing)} (as if the OSC message came in) -> {ran}",
            crate::osc::osc_trigger,
        ),
        // Lighting
        cmd(
            "dmx.get",
            "DMX monitor",
            "Lighting/DMX monitor",
            None,
            "{universe: 1-64} -> {universe, levels: [512 levels 0-255, channel 1 first]}",
            crate::dmx::dmx_get,
        ),
        cmd(
            "dmx.set",
            "Set DMX levels",
            "",
            None,
            "{universe: 1-64, channel: 1-512, value: 0-255 | values: [0-255, ...] from channel on} -> {changed}",
            crate::dmx::dmx_set,
        ),
        cmd(
            "dmx.clear",
            "All DMX levels to zero",
            "Lighting/All Levels to Zero",
            None,
            "{universe?: 1-64} (every universe without one)",
            crate::dmx::dmx_clear,
        ),
        cmd(
            "dmx.blackout",
            "Blackout",
            "Lighting/Blackout",
            None,
            "{on?: bool} (switches without) -> {on}",
            crate::dmx::dmx_blackout,
        ),
        cmd("dmx.master", "Lighting master", "", None, "{level: 0-1}", crate::dmx::dmx_master),
        cmd(
            "lighting.freeze",
            "Freeze lighting",
            "Lighting/Freeze",
            None,
            "{on?: bool} (holds fades and effects; switches without) -> {on}",
            crate::dmx::lighting_freeze,
        ),
        cmd(
            "lighting.blind",
            "Blind",
            "Lighting/Blind",
            None,
            "{on?: bool} (fixture changes and recalled presets stay off the lights; switches without) -> {on}",
            crate::dmx::lighting_blind,
        ),
        cmd(
            "dmx.output_add",
            "Add a DMX output",
            "",
            None,
            "{universe: 1-64, protocol: artnet | sacn | usb_pro, network_universe?: Art-Net 0-32767 | sACN 1-63999, address?: IPv4 (empty: everyone on the network) | serial port for usb_pro (COM3, /dev/ttyUSB0), priority?: sACN 0-200} -> {output: index}",
            crate::dmx::dmx_output_add,
        ),
        cmd("dmx.output_remove", "Remove a DMX output", "", None, "{output: index}", crate::dmx::dmx_output_remove),
        cmd("dmx.output_list", "List DMX outputs", "", None, "none", crate::dmx::dmx_output_list),
        cmd(
            "fixture.add",
            "Add fixtures",
            "",
            None,
            "{kind: see fixture.kinds | type: index, mode?: index | channels: [attribute, ...], name?, universe: 1-64, address?: 1-512 (the first free one without), count?: 1-512 in a row} -> {fixtures: [index]}",
            crate::fixtures::fixture_add,
        ),
        cmd("fixture.remove", "Remove a fixture", "", None, "{fixture: index}", crate::fixtures::fixture_remove),
        cmd(
            "fixture.place",
            "Place a fixture on the stage",
            "",
            None,
            "{fixture: index, x: 0-1 left to right, y: 0-1 back to front}",
            crate::fixtures::fixture_place,
        ),
        cmd(
            "fixture.stage",
            "Stage",
            "Lighting/Stage",
            None,
            "none -> [{name, place: [x, y] | null, colour: #rrggbb, beam: {direction: degrees, reach: 0-1} | null}]",
            crate::fixtures::fixture_stage,
        ),
        cmd(
            "fixture.move",
            "Patch a fixture elsewhere",
            "",
            None,
            "{fixture: index, universe: 1-64, address?: 1-512} -> {universe, address}",
            crate::fixtures::fixture_move,
        ),
        cmd("fixture.list", "Fixtures", "Lighting/Fixtures", None, "none", crate::fixtures::fixture_list),
        cmd("fixture.kinds", "Kinds of fixtures", "", None, "none", crate::fixtures::fixture_kinds),
        cmd(
            "fixture_type.import",
            "Import a fixture type",
            "",
            None,
            "{path: an Open Fixture Library .json file} -> {type: index, name, modes: [name]}",
            crate::fixture_library::fixture_type_import,
        ),
        cmd(
            "fixture_type.remove",
            "Remove a fixture type",
            "",
            None,
            "{type: index}",
            crate::fixture_library::fixture_type_remove,
        ),
        cmd("fixture_type.list", "List fixture types", "", None, "none", crate::fixture_library::fixture_type_list),
        cmd(
            "fixture_type.set",
            "Make a fixture type",
            "",
            None,
            "{name, modes: [{name?, channels: [attribute, ...]}]} (replaces the type of that name) -> {type: index, name, modes: [name]}",
            crate::fixture_library::fixture_type_set,
        ),
        cmd(
            "group.set",
            "Set a group",
            "",
            None,
            "{name, fixtures: [index]} (replaces the group of that name) -> {group: index}",
            crate::looks::group_set,
        ),
        cmd("group.remove", "Remove a group", "", None, "{group: index}", crate::looks::group_remove),
        cmd("group.list", "List groups", "", None, "none", crate::looks::group_list),
        cmd(
            "preset.store",
            "Store a preset",
            "",
            None,
            "{name, fixture?: index | fixtures?: [index] | group?: index} (every fixture without one; replaces the preset of that name) -> {preset: index, values}",
            crate::looks::preset_store,
        ),
        cmd(
            "preset.recall",
            "Recall a preset",
            "",
            None,
            "{preset: index, fade?: seconds 0-600} -> {changed, fade}",
            crate::looks::preset_recall,
        ),
        cmd("preset.remove", "Remove a preset", "", None, "{preset: index}", crate::looks::preset_remove),
        cmd("preset.list", "Presets", "Lighting/Presets", None, "none", crate::looks::preset_list),
        cmd("cue_list.add", "Add a cue list", "", None, "{name} -> {list: index}", crate::cues::cue_list_add),
        cmd("cue_list.remove", "Remove a cue list", "", None, "{list: index}", crate::cues::cue_list_remove),
        cmd("cue_list.list", "List cue lists", "", None, "none", crate::cues::cue_list_list),
        cmd(
            "cue.add",
            "Add a cue",
            "",
            None,
            "{list: index, preset: index, fade?: seconds 0-600, name?} -> {cue: index}",
            crate::cues::cue_add,
        ),
        cmd("cue.remove", "Remove a cue", "", None, "{list: index, cue: index}", crate::cues::cue_remove),
        cmd(
            "cue.go",
            "Go to the next cue",
            "Lighting/Next Cue",
            None,
            "{list?: index (the first list without)} -> {cue, name}",
            crate::cues::cue_go,
        ),
        cmd(
            "cue.back",
            "Back to the cue before",
            "Lighting/Cue Before",
            None,
            "{list?: index} -> {cue, name}",
            crate::cues::cue_back,
        ),
        cmd("cue.jump", "Go to a cue", "", None, "{list: index, cue: index} -> {cue, name}", crate::cues::cue_jump),
        cmd(
            "song.light_cue_add",
            "Fire a lighting cue at a song time",
            "",
            None,
            "{song?: id (the current song without), time: seconds | bar: number (from 1) + beat?: number (from 1), list: index, cue: index} -> {light_cue: index}",
            crate::cues::light_cue_add,
        ),
        cmd(
            "song.light_cue_move",
            "Move a song's lighting cue",
            "",
            None,
            "{song?: id, light_cue: index, time: seconds | bar: number (from 1) + beat?: number (from 1)} -> {light_cue: index}",
            crate::cues::light_cue_move,
        ),
        cmd(
            "song.light_cue_remove",
            "Remove a song's lighting cue",
            "",
            None,
            "{song?: id, light_cue: index}",
            crate::cues::light_cue_remove,
        ),
        cmd(
            "effect.set",
            "Set an effect",
            "",
            None,
            "{name, group: index, attribute, shape?: sine|square|saw|triangle, period?: seconds, beats?: per cycle at the song tempo, low?: 0-1, high?: 0-1, spread?: 0-1} (replaces the effect of that name) -> {effect: index}",
            crate::effects::effect_set,
        ),
        cmd("effect.remove", "Remove an effect", "", None, "{effect: index}", crate::effects::effect_remove),
        cmd("effect.list", "List effects", "", None, "none", crate::effects::effect_list),
        cmd(
            "effect.templates",
            "Ready-made effects",
            "",
            None,
            "none -> [{template, name, attribute, shape, period, beats, spread}]",
            crate::effects::effect_templates,
        ),
        cmd(
            "effect.from_template",
            "Effect from a ready-made one",
            "",
            None,
            "{template, group: index, name?} -> {effect: index}",
            crate::effects::effect_from_template,
        ),
        cmd("effect.start", "Start an effect", "", None, "{effect: index}", crate::effects::effect_start),
        cmd(
            "effect.stop",
            "Stop effects",
            "Lighting/Stop Effects",
            None,
            "{effect?: index} (every effect without one)",
            crate::effects::effect_stop,
        ),
        cmd(
            "fixture.set",
            "Set fixture attributes",
            "",
            None,
            "{fixture: index | fixtures: [index] | group: index, values: {intensity|red|green|blue|white|amber|uv|pan|tilt|strobe|colour_wheel|gobo|zoom|focus|other: 0-1}} -> {changed}",
            crate::fixtures::fixture_set,
        ),
        cmd(
            "macro.set",
            "Set macro",
            "",
            None,
            "{name: capitals, digits and _ ($NAME in lyrics calls it), commands: [id] (ones that need nothing more)} -> {name}",
            crate::macros::macro_set,
        ),
        cmd("macro.remove", "Remove macro", "", None, "{name: text}", crate::macros::macro_remove),
        cmd("macro.list", "List macros", "", None, "none", crate::macros::macro_list),
        cmd("macro.run", "Run macro", "", None, "{name: text}", crate::macros::macro_run),
        cmd(
            "pad.add",
            "Add pad",
            "",
            None,
            "{label: text, colour?: #rrggbb, command?: id (one that needs nothing more), macro?: name, fader?: master|song|bus, bus?: index, toggle?: metronome|loop_song|autoplay|blackout|freeze|blind, page?: index (the page shown without)} (a command, a macro, a fader or a toggle) -> {pad: index}",
            crate::pads::pad_add,
        ),
        cmd("pad.remove", "Remove pad", "", None, "{pad: index}", crate::pads::pad_remove),
        cmd(
            "pad.move",
            "Move pad to a page",
            "",
            None,
            "{pad: index, page: index} (to the end of that page) -> {pad: index}",
            crate::pads::pad_move,
        ),
        cmd(
            "pad.page",
            "Show pad page",
            "",
            None,
            "{page: index} (one past the last starts a new page) -> {page, pages}",
            crate::pads::pad_page,
        ),
        cmd("pad.page_next", "Next pad page", "Pads/Next Page", None, "none", crate::pads::pad_page_next),
        cmd("pad.page_before", "Pad page before", "Pads/Page Before", None, "none", crate::pads::pad_page_before),
        cmd("pad.list", "List pads", "", None, "none", crate::pads::pad_list),
        cmd("pad.press", "Press pad", "", None, "{pad: index} -> {ran}", crate::pads::pad_press),
        cmd("voice.prepare", "Speak cues now", "", None, "{song?: id}", crate::voice::voice_prepare),
        with_song(cmd(
            "metronome.count_in",
            "Play with count-in",
            "Metronome/Play with Count-in",
            None,
            "none",
            metronome_count_in,
        )),
        // Library
        cmd(
            "library.search",
            "Search songs",
            "",
            Some("Ctrl+F"),
            "{query: text} e.g. title:drive -live, artist:\"Ana Ruiz\" OR bpm:>120",
            library_search,
        ),
        // Playlists
        cmd("playlist.list", "List playlists", "", None, "none", playlist_list),
        cmd("playlist.create", "New playlist", "Playlist/New Playlist…", None, "{name: text}", playlist_create),
        cmd(
            "playlist.create_smart",
            "New smart playlist",
            "Playlist/New Smart Playlist…",
            None,
            "{name: text, match_all: bool, rules: [{field, op, value}]}",
            playlist_create_smart,
        ),
        cmd("playlist.rename", "Rename playlist", "", None, "{playlist: index, name: text}", playlist_rename),
        cmd("playlist.delete", "Delete playlist", "", None, "{playlist: index, confirm: true}", playlist_delete),
        with_open_playlist(cmd(
            "playlist.export_package",
            "Export playlist package",
            "Playlist/Export Playlist Package…",
            None,
            "{path: text, playlist?: index (the open playlist when left out)} (.actcraftpack: the playlist and its songs with their files)",
            crate::package_commands::playlist_export_package,
        )),
        cmd("playlist.select", "Play from playlist", "", None, "{playlist: index|null}", playlist_select),
        cmd("playlist.add_set", "Add set", "", None, "{playlist: index, name?: text}", playlist_add_set),
        cmd(
            "playlist.add_songs",
            "Add songs to playlist",
            "",
            None,
            "{playlist: index, set?: index, songs: [id]}",
            playlist_add_songs,
        ),
        cmd(
            "playlist.remove_songs",
            "Remove songs from playlist",
            "",
            None,
            "{playlist: index, set?: index, indexes: [index]}",
            playlist_remove_songs,
        ),
        cmd(
            "playlist.move_songs",
            "Move songs in playlist",
            "",
            None,
            "{playlist: index, set?: index, indexes: [index], to: index}",
            playlist_move_songs,
        ),
        cmd(
            "playlist.sorted_view",
            "Sorted view of a set",
            "",
            None,
            "{playlist: index, set?: index, by: title|artist|key|bpm|year|duration, descending?: bool}",
            playlist_sorted_view,
        ),
        cmd("playlist.songs", "Songs of a playlist", "", None, "{playlist: index}", playlist_songs_cmd),
        // Queue
        cmd("queue.list", "Show queue", "", None, "none", queue_list),
        cmd("queue.add", "Add to queue", "Queue/Add to Queue", None, "{song: id}", queue_add),
        cmd("queue.play_next", "Play next", "Queue/Play Next", None, "{song: id}", queue_play_next),
        cmd("queue.add_set", "Add set to queue", "", None, "{playlist: index, set?: index}", queue_add_set),
        cmd("queue.from_playlist", "Queue whole playlist", "", None, "{playlist: index}", queue_from_playlist),
        cmd("queue.remove", "Remove from queue", "", None, "{index: index}", queue_remove),
        cmd("queue.clear", "Clear queue", "Queue/Clear Queue", None, "none", queue_clear),
        cmd(
            "queue.set_double_tap",
            "Double-tap action",
            "",
            None,
            "{mode: load|play_next|add_to_queue|ignore}",
            queue_set_double_tap,
        ),
        cmd("queue.double_tap", "Double-tap a song", "", None, "{song: id}", queue_double_tap),
        // History
        cmd("history.list", "Play history", "", None, "none", history_list),
        cmd(
            "history.mark_done",
            "Mark song done",
            "",
            None,
            "{index?: index} (defaults to the newest)",
            history_mark_done,
        ),
        cmd(
            "history.playlist_from_date",
            "Playlist from a played date",
            "",
            None,
            "{date: YYYY-MM-DD, name?: text}",
            history_playlist_from_date,
        ),
        // Waveform
        with_songs(cmd(
            "waveform.peaks",
            "Waveform",
            "",
            None,
            "{song?: id, track: index, from?: seconds, to?: seconds, columns: 1..4096}",
            waveform_peaks,
        )),
    ]
}

/// Builds the registry. A registration failure is a programming error caught by the tests; at
/// run time the command is skipped rather than crashing.
pub fn build() -> Registry<Engine> {
    let mut r = Registry::new();
    for c in all() {
        let _ = r.register(c);
    }
    r
}

fn ok() -> CommandResult {
    Ok(Value::Null)
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn id_value(prm: &Value, key: &str) -> Result<u64, CommandError> {
    let n = p::i64_in(prm, key, 1, i64::MAX)?;
    u64::try_from(n).map_err(|_| invalid(format!("`{key}` must be a song id")))
}

fn song_exists(e: &Engine, id: u64) -> Result<u64, CommandError> {
    if e.show.song(id).is_some() { Ok(id) } else { Err(invalid(format!("no song with id {id}"))) }
}

/// The `song` param, or the current song.
fn song_param(e: &Engine, prm: &Value) -> Result<u64, CommandError> {
    if p::has(prm, "song") {
        song_exists(e, id_value(prm, "song")?)
    } else {
        e.current_song.filter(|&id| e.show.song(id).is_some()).ok_or_else(|| invalid("no song chosen"))
    }
}

fn track_param(e: &Engine, prm: &Value, song: u64) -> Result<usize, CommandError> {
    let len = e.show.song(song).map_or(0, |s| s.tracks.len());
    p::index(prm, "track", len)
}

fn playlist_param(e: &Engine, prm: &Value) -> Result<usize, CommandError> {
    p::index(prm, "playlist", e.show.playlists.len())
}

fn set_param(e: &Engine, prm: &Value, pl: usize) -> Result<usize, CommandError> {
    let len = e.show.playlists.get(pl).map_or(0, |x| x.sets.len());
    if p::has(prm, "set") {
        p::index(prm, "set", len)
    } else if len > 0 {
        Ok(0)
    } else {
        Err(invalid("the playlist has no sets"))
    }
}

fn song_ids(e: &Engine, prm: &Value, key: &str) -> Result<Vec<u64>, CommandError> {
    let Some(arr) = prm.get(key).and_then(Value::as_array) else {
        return Err(invalid(format!("`{key}` must be a list of song ids")));
    };
    if arr.len() > MAX_SET_SONGS {
        return Err(invalid(format!("`{key}` has more than {MAX_SET_SONGS} songs")));
    }
    arr.iter()
        .map(|v| {
            let id = v.as_u64().ok_or_else(|| invalid(format!("`{key}` must hold song ids")))?;
            song_exists(e, id)
        })
        .collect()
}

/// Songs of a playlist in play order (smart playlists are evaluated now).
pub fn playlist_songs(show: &Show, pl: &Playlist) -> Vec<u64> {
    match &pl.smart {
        Some(rules) => smart::evaluate(rules, &show.songs),
        None => pl.sets.iter().flat_map(|s| s.songs.iter().copied()).filter(|id| show.song(*id).is_some()).collect(),
    }
}

fn song_json(s: &Song) -> Value {
    json!({
        "id": s.id, "title": s.title, "artist": s.artist, "key": s.key, "bpm": s.bpm,
        "year": s.year, "duration": s.duration, "keywords": s.keywords, "tracks": s.tracks.len(),
    })
}

// ---- App ----

fn app_about(_: &mut Engine, _: &Value) -> CommandResult {
    Ok(json!({"name": "ActCraft", "version": env!("CARGO_PKG_VERSION"), "licence": "MIT OR Apache-2.0"}))
}

fn app_commands(e: &mut Engine, _: &Value) -> CommandResult {
    Ok(registry().describe(e))
}

fn app_messages(e: &mut Engine, _: &Value) -> CommandResult {
    Ok(Value::Array(e.messages.iter().map(|m| json!({"text": m.text, "error": m.error})).collect()))
}

// ---- Show ----

pub(crate) fn check_discard(e: &Engine, prm: &Value) -> Result<(), CommandError> {
    if e.dirty && !p::opt_bool(prm, "discard")?.unwrap_or(false) {
        return Err(failed("the show has unsaved changes; save it first or pass discard: true"));
    }
    Ok(())
}

pub(crate) fn replace_show(e: &mut Engine, show: Show, path: Option<String>) {
    e.forget_audio_state();
    e.show = show;
    e.show_path = path;
    e.dirty = false;
    e.current_song = None;
    e.current_playlist = None;
    e.queue.clear();
    e.search_results.clear();
    e.push_all_mixer();
}

fn show_new(e: &mut Engine, prm: &Value) -> CommandResult {
    check_discard(e, prm)?;
    replace_show(e, Show::default(), None);
    e.say("New show");
    ok()
}

fn show_open(e: &mut Engine, prm: &Value) -> CommandResult {
    let path = p::str_max(prm, "path", MAX_TEXT)?.to_owned();
    check_discard(e, prm)?;
    let bytes = e.services().read_file(&path).map_err(failed)?;
    let show = Show::from_bytes(&bytes).map_err(|err| failed(err.to_string()))?;
    let songs = show.songs.len();
    let first = show.songs.first().map(|s| s.id);
    replace_show(e, show, Some(path));
    // The first song is ready for playback; its audio starts decoding in the background now, so
    // Play doesn't have to wait for it.
    e.current_song = first;
    let files: Vec<String> = first
        .and_then(|id| e.show.song(id))
        .map(|s| s.tracks.iter().map(|t| t.file.clone()).collect())
        .unwrap_or_default();
    e.request_clips(files);
    e.say_with("Opened show with {count} songs", vec![crate::MessageArg::plain("count", songs)]);
    Ok(json!({"songs": songs}))
}

fn show_save(e: &mut Engine, prm: &Value) -> CommandResult {
    let path = match p::opt_str_max(prm, "path", MAX_TEXT)? {
        Some(path) => path.to_owned(),
        None => e.show_path.clone().ok_or_else(|| invalid("`path` is required for a show that was never saved"))?,
    };
    let bytes = e.show.to_bytes();
    e.services_mut().write_file(&path, &bytes).map_err(failed)?;
    e.show_path = Some(path.clone());
    e.dirty = false;
    e.say("Show saved");
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

fn show_save_as(e: &mut Engine, prm: &Value) -> CommandResult {
    p::str_max(prm, "path", MAX_TEXT)?;
    show_save(e, prm)
}

fn show_info(e: &mut Engine, _: &Value) -> CommandResult {
    Ok(json!({
        "name": e.show.name, "path": e.show_path, "dirty": e.dirty, "version": e.show.version,
        "songs": e.show.songs.len(), "playlists": e.show.playlists.len(), "audio": e.has_audio(),
        "current_song": e.current_song,
    }))
}

fn show_rename(e: &mut Engine, prm: &Value) -> CommandResult {
    e.show.name = p::str_max(prm, "name", MAX_TEXT)?.to_owned();
    e.mark_changed();
    ok()
}

// ---- Songs ----

fn song_list(e: &mut Engine, _: &Value) -> CommandResult {
    Ok(Value::Array(e.show.songs.iter().map(song_json).collect()))
}

fn apply_song_fields(s: &mut Song, prm: &Value) -> Result<(), CommandError> {
    // Read and check everything first, so a bad field changes nothing.
    let mut next = s.clone();
    if let Some(v) = p::opt_str_max(prm, "title", MAX_TEXT)? {
        next.title = v.to_owned();
    }
    if let Some(v) = p::opt_str_max(prm, "artist", MAX_TEXT)? {
        next.artist = v.to_owned();
    }
    if let Some(v) = p::opt_str_max(prm, "key", 16)? {
        next.key = v.to_owned();
    }
    if let Some(v) = p::opt_f64_in(prm, "bpm", 20.0, 400.0)? {
        next.bpm = v;
    }
    if let Some(v) = p::opt_i64_in(prm, "beats_per_bar", 1, 16)? {
        next.beats_per_bar = u8::try_from(v).unwrap_or(4);
    }
    if let Some(v) = p::opt_i64_in(prm, "year", 0, 9999)? {
        next.year = i32::try_from(v).ok();
    }
    if p::has(prm, "keywords") {
        next.keywords = p::str_list(prm, "keywords", 64, 256)?;
    }
    if let Some(v) = p::opt_i64_in(prm, "capo", 0, 12)? {
        next.capo = i8::try_from(v).unwrap_or(0);
    }
    if let Some(v) = p::opt_i64_in(prm, "transpose", -12, 12)? {
        next.transpose = i8::try_from(v).unwrap_or(0);
    }
    if let Some(v) = p::opt_f64_in(prm, "start", 0.0, 86_400.0)? {
        next.start = v;
    }
    if prm.get("end").is_some_and(Value::is_null) {
        next.end = None;
    } else if let Some(v) = p::opt_f64_in(prm, "end", 0.0, 86_400.0)? {
        if v <= next.start {
            return Err(invalid("`end` must be after `start`"));
        }
        next.end = Some(v);
    }
    if let Some(v) = p::opt_i64_in(prm, "count_in_bars", 0, 8)? {
        next.count_in_bars = u8::try_from(v).unwrap_or(0);
    }
    {
        use actcraft_show::limits::{MAX_SCORE_FIRST_BAR, MIN_SCORE_FIRST_BAR};
        if let Some(v) = p::opt_i64_in(prm, "score_first_bar", MIN_SCORE_FIRST_BAR.into(), MAX_SCORE_FIRST_BAR.into())?
        {
            next.score_first_bar = i32::try_from(v).unwrap_or(1);
        }
    }
    if let Some(v) = p::opt_str_max(prm, "lyrics_scroll", 32)? {
        next.lyrics_scroll = actcraft_show::LyricsScroll::parse(v)
            .ok_or_else(|| invalid(format!("`lyrics_scroll` must be line, smooth, constant or off, not `{v}`")))?;
    }
    if let Some(v) = p::opt_str_max(prm, "click", 32)? {
        next.click = actcraft_show::SongClick::parse(v).ok_or_else(|| {
            invalid(format!("`click` must be off, whole_song, count_in_only or visual_only, not `{v}`"))
        })?;
    }
    if let Some(v) = p::opt_f64_in(prm, "beat_anchor", -86_400.0, 86_400.0)? {
        next.beat_anchor = v;
    }
    if let Some(v) = p::opt_f64_in(prm, "volume_db", -96.0, 12.0)? {
        next.volume_db = v;
    }
    if let Some(v) = p::opt_bool(prm, "autoplay")? {
        next.autoplay = v;
    }
    if let Some(v) = p::opt_bool(prm, "loop_song")? {
        next.loop_song = v;
    }
    if let Some(v) = p::opt_bool(prm, "announce_title")? {
        next.announce_title = v;
    }
    if let Some(v) = p::opt_f64_in(prm, "fade_out", 0.0, 60.0)? {
        next.fade_out = v;
    }
    if prm.get("pdf_file").is_some_and(Value::is_null) {
        next.pdf_file = None;
    } else if let Some(v) = p::opt_str_max(prm, "pdf_file", MAX_TEXT)? {
        if services::extension(v).as_deref() != Some("pdf") {
            return Err(invalid("`pdf_file` must be a .pdf file"));
        }
        next.pdf_file = Some(v.to_owned());
    }
    if prm.get("notation_file").is_some_and(Value::is_null) {
        next.notation_file = None;
    } else if let Some(v) = p::opt_str_max(prm, "notation_file", MAX_TEXT)? {
        if !crate::notation_commands::is_notation_path(v) {
            return Err(invalid("`notation_file` must be a .musicxml, .xml or .mxl file"));
        }
        next.notation_file = Some(v.to_owned());
    }
    if prm.get("midi_file").is_some_and(Value::is_null) {
        next.midi_file = None;
    } else if let Some(v) = p::opt_str_max(prm, "midi_file", MAX_TEXT)? {
        if !crate::midi_file::is_midi_path(v) {
            return Err(invalid("`midi_file` must be a .mid, .midi or .smf file"));
        }
        next.midi_file = Some(v.to_owned());
    }
    if let Some(v) = p::opt_str_max(prm, "lyrics", actcraft_show::limits::MAX_LYRICS)? {
        next.lyrics = v.to_owned();
    }
    *s = next;
    Ok(())
}

fn song_add(e: &mut Engine, prm: &Value) -> CommandResult {
    if e.show.songs.len() >= MAX_SONGS {
        return Err(failed(format!("a show holds at most {MAX_SONGS} songs")));
    }
    let mut song = Song { id: e.show.next_song_id(), ..Song::default() };
    if prm.is_object() {
        apply_song_fields(&mut song, prm)?;
    }
    if song.title.is_empty() {
        song.title = format!("Song {}", e.show.songs.len() + 1);
    }
    let id = song.id;
    e.show.songs.push(song);
    e.mark_changed();
    if e.current_song.is_none() {
        e.current_song = Some(id);
    }
    Ok(json!({"song": id}))
}

fn song_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let Some(s) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    apply_song_fields(s, prm)?;
    e.mark_changed();
    e.push_song_settings(id);
    // The title, tempo or first beat place the spoken cues.
    if ["announce_title", "title", "bpm", "beats_per_bar", "beat_anchor", "start"].iter().any(|k| p::has(prm, k)) {
        e.voices_changed(id);
    }
    ok()
}

fn song_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_exists(e, id_value(prm, "song")?)?;
    if !p::opt_bool(prm, "confirm")?.unwrap_or(false) {
        return Err(failed("deleting a song needs confirm: true"));
    }
    e.show.songs.retain(|s| s.id != id);
    for pl in &mut e.show.playlists {
        for set in &mut pl.sets {
            set.songs.retain(|&s| s != id);
        }
    }
    e.queue.forget(id);
    if e.current_song == Some(id) {
        e.current_song = None;
        e.forget_audio_state();
    }
    e.mark_changed();
    ok()
}

fn song_load(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_exists(e, id_value(prm, "song")?)?;
    if e.playing() {
        return Err(failed("stop playback before loading another song, or crossfade to it"));
    }
    e.select_song(id);
    ok()
}

fn song_import_files(e: &mut Engine, prm: &Value) -> CommandResult {
    let mut paths = p::str_list(prm, "paths", 1000, MAX_TEXT)?;
    let format = p::opt_str_max(prm, "format", 256)?.map_or_else(|| e.filename_format.clone(), str::to_owned);
    filename::sort_alphabetical(&mut paths);
    let mut added = Vec::new();
    let mut skipped = Vec::new();
    // Index existing songs once so importing many files into a big library stays fast.
    let mut known: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for s in &e.show.songs {
        let (names, file) = filename::duplicate_keys(s);
        for k in [names.map(|n| format!("n:{n}")), file.map(|f| format!("f:{f}"))].into_iter().flatten() {
            known.entry(k).or_insert(s.id);
        }
    }
    for path in paths {
        if e.show.songs.len() >= MAX_SONGS {
            skipped.push(json!({"path": path, "why": "show is full"}));
            continue;
        }
        let name = services::file_name(&path).to_owned();
        let mut song = filename::parse(&name, &format).map_err(invalid)?;
        song.id = e.show.next_song_id();
        let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s).to_owned();
        song.tracks.push(Track { name: stem, file: path.clone(), ..Track::default() });
        let (names, file) = filename::duplicate_keys(&song);
        let keys: Vec<String> =
            [names.map(|n| format!("n:{n}")), file.map(|f| format!("f:{f}"))].into_iter().flatten().collect();
        if let Some(of) = keys.iter().find_map(|k| known.get(k)) {
            skipped.push(json!({"path": path, "why": "duplicate", "of": of}));
            continue;
        }
        for k in keys {
            known.insert(k, song.id);
        }
        added.push(song.id);
        e.show.songs.push(song);
    }
    if !added.is_empty() {
        e.mark_changed();
        if e.current_song.is_none() {
            e.current_song = added.first().copied();
        }
    }
    e.say_with(
        "Added {added} songs, skipped {skipped}",
        vec![crate::MessageArg::plain("added", added.len()), crate::MessageArg::plain("skipped", skipped.len())],
    );
    // Decode in the background (the window keeps responding) and estimate each tempo.
    e.queue_tempo_detection(added.iter().copied());
    Ok(json!({"added": added, "skipped": skipped}))
}

fn song_detect_bpm(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    e.queue_tempo_detection([id]);
    Ok(json!({"song": id, "bpm": e.show.song(id).map(|s| s.bpm)}))
}

/// Lyrics file types `lyrics.import` reads.
pub const LYRICS_EXTENSIONS: &[&str] = &["txt", "cho", "chopro", "chordpro", "crd", "pro", "lrc", "docx"];

/// Largest DOCX document read for its text (it is compressed; the text is capped on its own).
const MAX_DOCX_BYTES: usize = 32 * 1024 * 1024;

/// A web build reads no DOCX documents (see the engine's Cargo.toml).
#[cfg(target_arch = "wasm32")]
fn docx_text(_bytes: &[u8]) -> Result<String, CommandError> {
    Err(failed("DOCX documents are read in the desktop app"))
}

/// The words of a DOCX document's body, paragraph by paragraph, as lyrics text.
#[cfg(not(target_arch = "wasm32"))]
fn docx_text(bytes: &[u8]) -> Result<String, CommandError> {
    let doc = craft_docx::read(bytes).map_err(|_| failed("that DOCX document could not be read"))?;
    let text = doc.plain_text(craft_wordmodel::StoryRef::Body);
    // Table cells come apart by tabs; on a lyrics line they read best as spaces.
    let mut text = text.replace('\t', " ");
    if text.len() > actcraft_show::limits::MAX_LYRICS {
        let mut end = actcraft_show::limits::MAX_LYRICS;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Ok(text)
}

fn lyrics_import(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let path = p::str_max(prm, "path", MAX_TEXT)?.to_owned();
    let ext = services::extension(&path).unwrap_or_default();
    if !LYRICS_EXTENSIONS.contains(&ext.as_str()) {
        return Err(invalid(format!("ActCraft reads lyrics from {} files", LYRICS_EXTENSIONS.join(", "))));
    }
    let bytes = e.services().read_file(&path).map_err(failed)?;
    let limit = if ext == "docx" { MAX_DOCX_BYTES } else { actcraft_show::limits::MAX_LYRICS };
    if bytes.len() > limit {
        return Err(failed("that lyrics file is too large"));
    }
    let text = if ext == "docx" { docx_text(&bytes)? } else { actcraft_lyrics::decode_text(&bytes) };
    let parsed = actcraft_lyrics::parse(&text);
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    // ChordPro files may name the song; fill in only what's still empty.
    for (field, slot) in [("title", &mut song.title), ("artist", &mut song.artist), ("key", &mut song.key)] {
        if let Some(v) = parsed.directive(field)
            && (slot.trim().is_empty() || (field == "title" && slot.starts_with("Song ")))
        {
            *slot = v.to_owned();
        }
    }
    song.lyrics = text;
    let lines = parsed.lines.len();
    e.mark_changed();
    e.say_with("Imported lyrics: {lines} lines", vec![crate::MessageArg::plain("lines", lines)]);
    Ok(json!({"lines": lines, "timecodes": parsed.has_timecodes()}))
}

fn lyrics_stamp_line(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let line = usize::try_from(p::i64_in(prm, "line", 0, actcraft_lyrics::MAX_LINES as i64)?).unwrap_or(0);
    let seconds = p::opt_f64_in(prm, "seconds", 0.0, 86_399.0)?.unwrap_or(e.status.time.max(0.0));
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    let updated = actcraft_lyrics::stamp_line(&song.lyrics, line, seconds)
        .ok_or_else(|| invalid("the lyrics have no such line"))?;
    song.lyrics = updated;
    e.mark_changed();
    Ok(json!({"line": line, "seconds": seconds}))
}

/// Replaces text everywhere in a song's lyrics.
fn lyrics_replace(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let find = p::str_max(prm, "find", MAX_TEXT)?;
    if find.is_empty() {
        return Err(invalid("`find` must not be empty"));
    }
    let replacement = p::opt_str_max(prm, "replace", MAX_TEXT)?.unwrap_or("");
    let match_case = p::opt_bool(prm, "match_case")?.unwrap_or(false);
    let Some(song) = e.show.song(id) else { return Err(invalid("no such song")) };
    let (updated, count) = actcraft_lyrics::replace_all(&song.lyrics, find, replacement, match_case);
    if updated.len() > actcraft_show::limits::MAX_LYRICS {
        return Err(failed("the lyrics would get too long"));
    }
    if count > 0 {
        if let Some(song) = e.show.song_mut(id) {
            song.lyrics = updated;
        }
        e.mark_changed();
    }
    Ok(json!({"replaced": count}))
}

fn library_set_filename_format(e: &mut Engine, prm: &Value) -> CommandResult {
    let format = p::str_max(prm, "format", 256)?;
    // Check the format on a sample name so a typo is caught now, not at the next import.
    filename::parse("Artist - Title.wav", format).map_err(invalid)?;
    e.filename_format = format.to_owned();
    ok()
}

fn song_duplicates(e: &mut Engine, _: &Value) -> CommandResult {
    let pairs = filename::find_duplicates(&e.show.songs, 1000);
    Ok(Value::Array(pairs.into_iter().map(|(a, b)| json!([a, b])).collect()))
}

// ---- Tracks ----

fn track_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let name = p::opt_str_max(prm, "name", MAX_TEXT)?.map(str::to_owned);
    let file = p::opt_str_max(prm, "file", MAX_TEXT)?.map(str::to_owned);
    let Some(s) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    if s.tracks.len() >= MAX_TRACKS {
        return Err(failed(format!("a song holds at most {MAX_TRACKS} tracks")));
    }
    let n = s.tracks.len();
    s.tracks.push(Track {
        name: name.unwrap_or_else(|| format!("Track {}", n + 1)),
        file: file.unwrap_or_default(),
        ..Track::default()
    });
    e.mark_changed();
    e.reload_if_loaded(id);
    Ok(json!({"track": n}))
}

fn track_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let t = track_param(e, prm, id)?;
    if let Some(s) = e.show.song_mut(id) {
        s.tracks.remove(t);
    }
    e.mark_changed();
    e.reload_if_loaded(id);
    ok()
}

fn track_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let t = track_param(e, prm, id)?;
    let name = p::opt_str_max(prm, "name", MAX_TEXT)?;
    let colour = p::opt_str_max(prm, "colour", 16)?;
    if let Some(c) = colour
        && parse_colour(c).is_none()
    {
        return Err(invalid("`colour` must look like #rrggbb"));
    }
    let file = p::opt_str_max(prm, "file", MAX_TEXT)?;
    let volume = p::opt_f64_in(prm, "volume_db", -96.0, 12.0)?;
    let pan = p::opt_f64_in(prm, "pan", -1.0, 1.0)?;
    let mute = p::opt_bool(prm, "mute")?;
    let bus = p::opt_i64_in(prm, "bus", 0, (BUSES - 1) as i64)?;
    let m2s = p::opt_bool(prm, "mono_to_stereo")?;
    let Some(track) = e.show.song_mut(id).and_then(|s| s.tracks.get_mut(t)) else {
        return Err(invalid("no such track"));
    };
    if let Some(v) = name {
        track.name = v.to_owned();
    }
    if let Some(v) = colour {
        track.colour = v.to_owned();
    }
    let file_changed = file.is_some_and(|f| f != track.file);
    if let Some(v) = file {
        track.file = v.to_owned();
    }
    if let Some(v) = volume {
        track.volume_db = v;
    }
    if let Some(v) = pan {
        track.pan = v;
    }
    if let Some(v) = mute {
        track.mute = v;
    }
    if let Some(v) = bus {
        track.bus = u8::try_from(v).unwrap_or(0);
    }
    if let Some(v) = m2s {
        track.mono_to_stereo = v;
    }
    e.mark_changed();
    if file_changed {
        e.reload_if_loaded(id);
    } else {
        e.push_track(id, t);
    }
    ok()
}

fn track_set_eq(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let t = track_param(e, prm, id)?;
    let band = p::index(prm, "band", 4)?;
    let freq = p::opt_f64_in(prm, "freq", 20.0, 20_000.0)?;
    let gain = p::opt_f64_in(prm, "gain_db", -24.0, 24.0)?;
    let q = p::opt_f64_in(prm, "q", 0.1, 18.0)?;
    let Some(b) = e.show.song_mut(id).and_then(|s| s.tracks.get_mut(t)).and_then(|tr| tr.eq.get_mut(band)) else {
        return Err(invalid("no such track"));
    };
    if let Some(v) = freq {
        b.freq = v;
    }
    if let Some(v) = gain {
        b.gain_db = v;
    }
    if let Some(v) = q {
        b.q = v;
    }
    e.mark_changed();
    e.push_track(id, t);
    ok()
}

fn track_mute_group(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let t = track_param(e, prm, id)?;
    let g = p::i64_in(prm, "group", 1, MUTE_GROUPS as i64)?;
    let member = p::bool(prm, "member")?;
    let bit = 1u8 << (g - 1).clamp(0, 7);
    let Some(track) = e.show.song_mut(id).and_then(|s| s.tracks.get_mut(t)) else {
        return Err(invalid("no such track"));
    };
    if member {
        track.mute_groups |= bit;
    } else {
        track.mute_groups &= !bit;
    }
    e.mark_changed();
    e.push_track(id, t);
    ok()
}

// ---- Mixer ----

fn bus_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let b = p::index(prm, "bus", e.show.buses.len())?;
    let name = p::opt_str_max(prm, "name", MAX_TEXT)?;
    let vol = p::opt_f64_in(prm, "volume_db", -96.0, 12.0)?;
    let mute = p::opt_bool(prm, "mute")?;
    let output = p::opt_i64_in(prm, "output", 0, 255)?;
    let Some(bus) = e.show.buses.get_mut(b) else { return Err(invalid("no such bus")) };
    if let Some(v) = name {
        bus.name = v.to_owned();
    }
    if let Some(v) = vol {
        bus.volume_db = v;
    }
    if let Some(v) = mute {
        bus.mute = v;
    }
    if let Some(v) = output {
        bus.output = u16::try_from(v).unwrap_or(0);
    }
    e.mark_changed();
    e.push_bus(b);
    ok()
}

fn mixer_master(e: &mut Engine, prm: &Value) -> CommandResult {
    e.show.mixer.master_volume_db = p::f64_in(prm, "db", -96.0, 12.0)?;
    let g = actcraft_audio::dsp::db_to_gain(e.show.mixer.master_volume_db);
    e.send(Msg::MasterGain(g));
    e.mark_changed();
    ok()
}

fn mixer_song(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let db = p::f64_in(prm, "db", -96.0, 12.0)?;
    if let Some(s) = e.show.song_mut(id) {
        s.volume_db = db;
    }
    e.mark_changed();
    e.push_song_settings(id);
    ok()
}

fn group_index(prm: &Value) -> Result<usize, CommandError> {
    let g = p::i64_in(prm, "group", 1, MUTE_GROUPS as i64)?;
    usize::try_from(g - 1).map_err(|_| invalid("bad group"))
}

fn mute_group_toggle(e: &mut Engine, prm: &Value) -> CommandResult {
    let g = group_index(prm)?;
    let Some(group) = e.show.mute_groups.get_mut(g) else { return Err(invalid("no such group")) };
    group.active = !group.active;
    let (label, active) = (group.label.clone(), group.active);
    e.push_mute_groups();
    e.say_with(
        if active { "{group} muted" } else { "{group} unmuted" },
        vec![crate::MessageArg::plain("group", label)],
    );
    Ok(json!({"active": active}))
}

fn mute_group_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let g = group_index(prm)?;
    let active = p::opt_bool(prm, "active")?;
    let label = p::opt_str_max(prm, "label", MAX_TEXT)?;
    let Some(group) = e.show.mute_groups.get_mut(g) else { return Err(invalid("no such group")) };
    if let Some(v) = active {
        group.active = v;
    }
    if let Some(v) = label {
        group.label = v.to_owned();
    }
    e.mark_changed();
    e.push_mute_groups();
    ok()
}

// ---- Transport ----

fn need_audio(e: &Engine) -> Result<(), CommandError> {
    if e.has_audio() { Ok(()) } else { Err(failed("no audio output is running")) }
}

fn start_playback(e: &mut Engine, count_in: bool) -> CommandResult {
    need_audio(e)?;
    let id = song_param(e, &Value::Null)?;
    if e.playing() {
        return Ok(json!({"playing": true}));
    }
    if e.transport_guard() {
        return Ok(json!({"ignored": "double trigger"}));
    }
    if e.loaded_song() != Some(id) {
        e.load_into_renderer(id);
    }
    let now = e.services().now();
    if e.is_loading(id) {
        // Its files are still decoding: it starts by itself when they are ready.
        e.play_when_loaded(count_in);
        history::record(&mut e.show.history, id, now);
        e.say("Loading audio; playback starts when it is ready");
        return Ok(json!({"playing": false, "loading": true}));
    }
    let quantized = e.show.metronome.quantized_start;
    if !e.send(Msg::Play { count_in, quantized }) {
        return Err(failed("the audio output is busy; try again"));
    }
    history::record(&mut e.show.history, id, now);
    Ok(json!({"playing": true}))
}

fn transport_play(e: &mut Engine, prm: &Value) -> CommandResult {
    let count_in = p::opt_bool(prm, "count_in")?.unwrap_or(true);
    if let Some(from) = p::opt_f64_in(prm, "from", 0.0, 86_400.0)? {
        // Start somewhere else (to hear a cue): the song must be loaded so the place sticks.
        need_audio(e)?;
        let id = song_param(e, &Value::Null)?;
        if e.loaded_song() != Some(id) {
            e.load_into_renderer(id);
        }
        if e.is_loading(id) {
            return Err(failed("that song's audio is still loading; try again in a moment"));
        }
        // Stop, move, play: sent together, so the status of the last frame doesn't matter.
        e.send(Msg::Stop);
        e.send(Msg::Seek(from));
        if !e.send(Msg::Play { count_in, quantized: false }) {
            return Err(failed("the audio output is busy; try again"));
        }
        return Ok(json!({"playing": true, "from": from}));
    }
    start_playback(e, count_in)
}

/// Stops where playback is; Play goes on from there.
fn transport_pause(e: &mut Engine, _: &Value) -> CommandResult {
    need_audio(e)?;
    if e.transport_guard() {
        return Ok(json!({"ignored": "double trigger"}));
    }
    e.send(Msg::Stop);
    Ok(json!({"seconds": e.status.time}))
}

/// Stops and goes back to the song's start, so Play starts the song from the top.
fn transport_stop(e: &mut Engine, _: &Value) -> CommandResult {
    need_audio(e)?;
    if e.transport_guard() {
        return Ok(json!({"ignored": "double trigger"}));
    }
    e.send(Msg::Stop);
    let start = e.loaded_song_pub().and_then(|id| e.show.song(id)).map_or(0.0, |s| s.start.max(0.0));
    e.send(Msg::Seek(start));
    Ok(json!({"seconds": start}))
}

fn transport_toggle(e: &mut Engine, _: &Value) -> CommandResult {
    if e.playing() { transport_pause(e, &Value::Null) } else { start_playback(e, true) }
}

fn transport_seek(e: &mut Engine, prm: &Value) -> CommandResult {
    need_audio(e)?;
    let t = p::f64_in(prm, "seconds", 0.0, 86_400.0)?;
    e.send(Msg::Seek(t));
    ok()
}

fn change_song(e: &mut Engine, target: Option<u64>) -> CommandResult {
    let Some(id) = target else { return Err(failed("no song in that direction")) };
    let crossfade = e.show.transitions.crossfade;
    if e.playing() && crossfade > 0.0 {
        return crossfade_to(e, id, crossfade);
    }
    let was_playing = e.playing();
    if was_playing {
        e.send(Msg::Stop);
    }
    e.select_song(id);
    Ok(json!({"song": id}))
}

fn transport_next(e: &mut Engine, _: &Value) -> CommandResult {
    let next = e.next_song_id();
    change_song(e, next)
}

fn transport_previous(e: &mut Engine, _: &Value) -> CommandResult {
    let prev = e.neighbour(-1);
    change_song(e, prev)
}

fn crossfade_to(e: &mut Engine, id: u64, seconds: f64) -> CommandResult {
    need_audio(e)?;
    if e.status.crossfading {
        return Err(failed("a crossfade is already running; request ignored"));
    }
    let files: Vec<String> =
        e.show.song(id).map(|s| s.tracks.iter().map(|t| t.file.clone()).collect()).unwrap_or_default();
    e.request_clips(files);
    if e.missing_files_pub(id) {
        return Err(failed("that song's audio is still loading; try again in a moment"));
    }
    let Some(load) = e.song_load(id) else { return Err(invalid("no such song")) };
    if !e.send(Msg::Crossfade { next: Box::new(load), seconds }) {
        return Err(failed("the audio output is busy; try again"));
    }
    e.current_song = Some(id);
    e.mark_loaded(id);
    let now = e.services().now();
    history::record(&mut e.show.history, id, now);
    Ok(json!({"song": id}))
}

fn transport_crossfade_to(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_exists(e, id_value(prm, "song")?)?;
    let secs = p::opt_f64_in(prm, "seconds", 0.0, 30.0)?.unwrap_or(e.show.transitions.crossfade);
    if !e.playing() {
        e.select_song(id);
        return start_playback(e, false);
    }
    crossfade_to(e, id, secs)
}

fn transport_set_crossfade(e: &mut Engine, prm: &Value) -> CommandResult {
    e.show.transitions.crossfade = p::f64_in(prm, "seconds", 0.0, 30.0)?;
    e.mark_changed();
    ok()
}

fn transport_set_guard(e: &mut Engine, prm: &Value) -> CommandResult {
    e.play_guard_ms = u32::try_from(p::i64_in(prm, "ms", 0, 5000)?).unwrap_or(0);
    ok()
}

// ---- Metronome ----

fn metronome_toggle(e: &mut Engine, _: &Value) -> CommandResult {
    e.show.metronome.enabled = !e.show.metronome.enabled;
    let on = e.show.metronome.enabled;
    e.push_metronome();
    e.say(if on { "Metronome on" } else { "Metronome off" });
    Ok(json!({"enabled": on}))
}

fn metronome_cycle(e: &mut Engine, _: &Value) -> CommandResult {
    // With a song chosen, its own click setting cycles; otherwise the show's default mode.
    if let Some(song) = e.current_song.and_then(|id| e.show.song_mut(id)) {
        song.click = song.click.next();
        let label = song.click.label();
        e.mark_changed();
        e.push_metronome();
        e.say_with("Click for this song: {mode}", vec![crate::MessageArg::word("mode", label)]);
        return Ok(json!({"click": label}));
    }
    e.show.metronome.mode = e.show.metronome.mode.next();
    let label = e.show.metronome.mode.label();
    e.push_metronome();
    e.say_with("Metronome: {mode}", vec![crate::MessageArg::word("mode", label)]);
    Ok(json!({"mode": label}))
}

fn metronome_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let enabled = p::opt_bool(prm, "enabled")?;
    let mode = match p::opt_str_max(prm, "mode", 32)? {
        None => None,
        Some("audible") => Some(MetronomeMode::Audible),
        Some("count_in_only") => Some(MetronomeMode::CountInOnly),
        Some("visual_only") => Some(MetronomeMode::VisualOnly),
        Some(other) => return Err(invalid(format!("unknown metronome mode `{other}`"))),
    };
    let vol = p::opt_f64_in(prm, "volume_db", -96.0, 12.0)?;
    let bus = p::opt_i64_in(prm, "bus", 0, (BUSES - 1) as i64)?;
    let quant = p::opt_bool(prm, "quantized_start")?;
    let free = p::opt_bool(prm, "run_while_stopped")?;
    let lat = p::opt_f64_in(prm, "latency_offset_ms", -500.0, 500.0)?;
    let m = &mut e.show.metronome;
    if let Some(v) = enabled {
        m.enabled = v;
    }
    if let Some(v) = mode {
        m.mode = v;
    }
    if let Some(v) = vol {
        m.volume_db = v;
    }
    if let Some(v) = bus {
        m.bus = u8::try_from(v).unwrap_or(0);
    }
    if let Some(v) = quant {
        m.quantized_start = v;
    }
    if let Some(v) = free {
        m.run_while_stopped = v;
    }
    if let Some(v) = lat {
        m.latency_offset_ms = v;
    }
    e.mark_changed();
    e.push_metronome();
    ok()
}

fn metronome_count_in(e: &mut Engine, _: &Value) -> CommandResult {
    start_playback(e, true)
}

// ---- Library ----

fn library_search(e: &mut Engine, prm: &Value) -> CommandResult {
    let q = p::str_max(prm, "query", search::MAX_QUERY_BYTES)?;
    let query = Query::parse(q).map_err(invalid)?;
    let ids = search::search(&e.show.songs, &query);
    e.search_query = q.to_owned();
    e.search_results = ids.clone();
    Ok(json!(ids))
}

// ---- Playlists ----

fn playlist_list(e: &mut Engine, _: &Value) -> CommandResult {
    let list = e
        .show
        .playlists
        .iter()
        .enumerate()
        .map(|(i, pl)| {
            json!({"playlist": i, "name": pl.name, "smart": pl.smart.is_some(),
                "sets": pl.sets.iter().map(|s| json!({"name": s.name, "songs": s.songs})).collect::<Vec<_>>()})
        })
        .collect();
    Ok(Value::Array(list))
}

fn playlist_create(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", MAX_TEXT)?.to_owned();
    if e.show.playlists.len() >= MAX_PLAYLISTS {
        return Err(failed(format!("a show holds at most {MAX_PLAYLISTS} playlists")));
    }
    e.show.playlists.push(Playlist {
        name,
        sets: vec![Set { name: "Set 1".to_owned(), ..Set::default() }],
        ..Playlist::default()
    });
    e.mark_changed();
    Ok(json!({"playlist": e.show.playlists.len() - 1}))
}

fn playlist_create_smart(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", MAX_TEXT)?.to_owned();
    let match_all = p::bool(prm, "match_all")?;
    let Some(arr) = prm.get("rules").and_then(Value::as_array) else { return Err(invalid("`rules` must be a list")) };
    if arr.is_empty() || arr.len() > actcraft_show::limits::MAX_RULES {
        return Err(invalid("give 1 to 32 rules"));
    }
    let mut rules = Vec::new();
    for r in arr {
        let rule = SmartRule {
            field: p::str_max(r, "field", 32)?.to_owned(),
            op: p::str_max(r, "op", 8)?.to_owned(),
            value: p::str_max(r, "value", MAX_TEXT)?.to_owned(),
        };
        smart::validate(&rule).map_err(invalid)?;
        rules.push(rule);
    }
    if e.show.playlists.len() >= MAX_PLAYLISTS {
        return Err(failed(format!("a show holds at most {MAX_PLAYLISTS} playlists")));
    }
    e.show.playlists.push(Playlist {
        name,
        sets: Vec::new(),
        smart: Some(SmartRules { match_all, rules }),
        ..Playlist::default()
    });
    e.mark_changed();
    Ok(json!({"playlist": e.show.playlists.len() - 1}))
}

fn playlist_rename(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = playlist_param(e, prm)?;
    let name = p::str_max(prm, "name", MAX_TEXT)?.to_owned();
    if let Some(pl) = e.show.playlists.get_mut(i) {
        pl.name = name;
    }
    e.mark_changed();
    ok()
}

fn playlist_delete(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = playlist_param(e, prm)?;
    if !p::opt_bool(prm, "confirm")?.unwrap_or(false) {
        return Err(failed("deleting a playlist needs confirm: true"));
    }
    e.show.playlists.remove(i);
    e.current_playlist = match e.current_playlist {
        Some(c) if c == i => None,
        Some(c) if c > i => Some(c - 1),
        other => other,
    };
    e.mark_changed();
    ok()
}

fn playlist_select(e: &mut Engine, prm: &Value) -> CommandResult {
    e.current_playlist = if p::has(prm, "playlist") { Some(playlist_param(e, prm)?) } else { None };
    ok()
}

fn playlist_add_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = playlist_param(e, prm)?;
    let name = p::opt_str_max(prm, "name", MAX_TEXT)?.map(str::to_owned);
    let Some(pl) = e.show.playlists.get_mut(i) else { return Err(invalid("no such playlist")) };
    if pl.smart.is_some() {
        return Err(failed("a smart playlist has no sets"));
    }
    if pl.sets.len() >= MAX_SETS {
        return Err(failed(format!("a playlist holds at most {MAX_SETS} sets")));
    }
    let n = pl.sets.len();
    pl.sets.push(Set { name: name.unwrap_or_else(|| format!("Set {}", n + 1)), ..Set::default() });
    e.mark_changed();
    Ok(json!({"set": n}))
}

fn playlist_add_songs(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = playlist_param(e, prm)?;
    let s = set_param(e, prm, i)?;
    let ids = song_ids(e, prm, "songs")?;
    let Some(set) = e.show.playlists.get_mut(i).and_then(|pl| pl.sets.get_mut(s)) else {
        return Err(invalid("no such set"));
    };
    if set.songs.len() + ids.len() > MAX_SET_SONGS {
        return Err(failed(format!("a set holds at most {MAX_SET_SONGS} songs")));
    }
    set.songs.extend(ids);
    e.mark_changed();
    ok()
}

fn playlist_remove_songs(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = playlist_param(e, prm)?;
    let s = set_param(e, prm, i)?;
    let len = e.show.playlists.get(i).and_then(|pl| pl.sets.get(s)).map_or(0, |x| x.songs.len());
    let mut idx = p::index_list(prm, "indexes", len, MAX_SET_SONGS)?;
    idx.sort_unstable();
    idx.dedup();
    if let Some(set) = e.show.playlists.get_mut(i).and_then(|pl| pl.sets.get_mut(s)) {
        for k in idx.into_iter().rev() {
            if k < set.songs.len() {
                set.songs.remove(k);
            }
        }
    }
    e.mark_changed();
    ok()
}

fn playlist_move_songs(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = playlist_param(e, prm)?;
    let s = set_param(e, prm, i)?;
    let len = e.show.playlists.get(i).and_then(|pl| pl.sets.get(s)).map_or(0, |x| x.songs.len());
    let idx = p::index_list(prm, "indexes", len, MAX_SET_SONGS)?;
    let to = usize::try_from(p::i64_in(prm, "to", 0, i64::try_from(len).unwrap_or(0))?).unwrap_or(0);
    if let Some(set) = e.show.playlists.get_mut(i).and_then(|pl| pl.sets.get_mut(s)) {
        set.songs = order::move_items(&set.songs, &idx, to);
    }
    e.mark_changed();
    ok()
}

fn playlist_sorted_view(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = playlist_param(e, prm)?;
    let s = set_param(e, prm, i)?;
    let by = p::str_max(prm, "by", 16)?;
    let key = SortKey::parse(by).ok_or_else(|| invalid(format!("cannot sort by `{by}`")))?;
    let desc = p::opt_bool(prm, "descending")?.unwrap_or(false);
    let ids = e.show.playlists.get(i).and_then(|pl| pl.sets.get(s)).map(|x| x.songs.clone()).unwrap_or_default();
    Ok(json!(order::sorted_view(&ids, &e.show.songs, key, desc)))
}

fn playlist_songs_cmd(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = playlist_param(e, prm)?;
    let ids = e.show.playlists.get(i).map(|pl| playlist_songs(&e.show, pl)).unwrap_or_default();
    Ok(json!(ids))
}

// ---- Queue ----

fn queue_list(e: &mut Engine, _: &Value) -> CommandResult {
    Ok(json!(e.queue.iter().copied().collect::<Vec<_>>()))
}

fn queue_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_exists(e, id_value(prm, "song")?)?;
    if !e.queue.push(id) {
        return Err(failed(format!("the queue holds at most {} songs", actcraft_library::queue::MAX_QUEUE)));
    }
    ok()
}

fn queue_play_next(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_exists(e, id_value(prm, "song")?)?;
    e.queue.play_next(id);
    ok()
}

fn queue_add_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = playlist_param(e, prm)?;
    let s = set_param(e, prm, i)?;
    let ids = e.show.playlists.get(i).and_then(|pl| pl.sets.get(s)).map(|x| x.songs.clone()).unwrap_or_default();
    let added = e.queue.extend(ids);
    Ok(json!({"added": added}))
}

fn queue_from_playlist(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = playlist_param(e, prm)?;
    let ids = e.show.playlists.get(i).map(|pl| playlist_songs(&e.show, pl)).unwrap_or_default();
    let added = e.queue.fill(ids);
    e.current_playlist = Some(i);
    Ok(json!({"added": added}))
}

fn queue_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "index", e.queue.len())?;
    e.queue.remove(i);
    ok()
}

fn queue_clear(e: &mut Engine, _: &Value) -> CommandResult {
    e.queue.clear();
    ok()
}

fn queue_set_double_tap(e: &mut Engine, prm: &Value) -> CommandResult {
    let mode = p::str_max(prm, "mode", 32)?;
    e.double_tap = DoubleTap::parse(mode).ok_or_else(|| invalid(format!("unknown double-tap mode `{mode}`")))?;
    ok()
}

fn queue_double_tap(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_exists(e, id_value(prm, "song")?)?;
    match e.double_tap {
        DoubleTap::Load => {
            if e.playing() {
                return Err(failed("stop playback before loading another song"));
            }
            e.select_song(id);
        }
        DoubleTap::PlayNext => e.queue.play_next(id),
        DoubleTap::AddToQueue => {
            e.queue.push(id);
        }
        DoubleTap::Ignore => {}
    }
    ok()
}

// ---- History ----

fn history_list(e: &mut Engine, _: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show.history.iter().map(|h| json!({"song": h.song, "played_at": h.played_at, "done": h.done})).collect(),
    ))
}

fn history_mark_done(e: &mut Engine, prm: &Value) -> CommandResult {
    let len = e.show.history.len();
    if len == 0 {
        return Err(failed("nothing has been played yet"));
    }
    let i = if p::has(prm, "index") { p::index(prm, "index", len)? } else { len - 1 };
    if let Some(h) = e.show.history.get_mut(i) {
        h.done = true;
    }
    e.mark_changed();
    ok()
}

fn history_playlist_from_date(e: &mut Engine, prm: &Value) -> CommandResult {
    let date = history::parse_date(p::str_max(prm, "date", 32)?).map_err(invalid)?;
    let name = p::opt_str_max(prm, "name", MAX_TEXT)?.map_or_else(|| format!("Played {date}"), str::to_owned);
    let ids = history::songs_on(&e.show.history, date);
    if ids.is_empty() {
        return Err(failed(format!("nothing was played on {date}")));
    }
    if e.show.playlists.len() >= MAX_PLAYLISTS {
        return Err(failed(format!("a show holds at most {MAX_PLAYLISTS} playlists")));
    }
    let n = ids.len();
    e.show.playlists.push(Playlist {
        name,
        sets: vec![Set { name: "Set 1".to_owned(), songs: ids, ..Set::default() }],
        ..Playlist::default()
    });
    e.mark_changed();
    Ok(json!({"playlist": e.show.playlists.len() - 1, "songs": n}))
}

// ---- Waveform ----

fn waveform_peaks(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_param(e, prm)?;
    let t = track_param(e, prm, id)?;
    let columns = usize::try_from(p::i64_in(prm, "columns", 1, 4096)?).unwrap_or(1);
    let file = e.show.song(id).and_then(|s| s.tracks.get(t)).map(|tr| tr.file.clone()).unwrap_or_default();
    if file.is_empty() {
        return Err(failed("the track has no audio file"));
    }
    let peaks = e.peaks(&file).map_err(failed)?;
    let len =
        peaks.pairs.len() as f64 * actcraft_audio::peaks::BASE_BUCKET as f64 / f64::from(peaks.sample_rate.max(1));
    let from = p::opt_f64_in(prm, "from", 0.0, 86_400.0)?.unwrap_or(0.0);
    let to = p::opt_f64_in(prm, "to", 0.0, 86_400.0)?.unwrap_or(len);
    if to <= from {
        return Err(invalid("`to` must be after `from`"));
    }
    let pairs = peaks.window(from, to, columns);
    Ok(json!({"seconds": len, "pairs": pairs.iter().map(|(a, b)| [a, b]).collect::<Vec<_>>()}))
}
