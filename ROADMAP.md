# Roadmap

Milestones run in order. Each one ends green on `cargo xtask ci`, with
[docs/parity.md](docs/parity.md) (every feature with its command id and status) updated.

Legend: ✅ done · 🔶 partly done · ⬜ not started

| Milestone | What it brings | Status |
|---|---|---|
| M0 | Foundation: commands, CLI, control channel, MCP, build gate | ✅ |
| M1 | Audio engine, mixer, outputs, metronome, library, playlists, queue | 🔶 |
| M2 | Regions, lyrics and chords, sheet music (PDF), lyrics editor, second screen | 🔶 in progress |
| M3 | MIDI in/out/sync, macros, remote actions, assignable on-screen controls | ⬜ |
| M4 | DMX output (Art-Net, sACN, USB), fixtures and patch, fixture library, DMX monitor | ⬜ |
| M5 | 2D stage preview, programming, presets, sequences, effects engine | ⬜ |
| M6 | Cues, cue lists, control pages, blind and freeze, beat sync | ⬜ |
| M7 | One show: lighting lane on the song timeline, shared clock | ⬜ |
| M8 | Band network, backup machine, OSC, DMX input, joystick, Hue | ⬜ |
| M9 | Installers: MSI and zip, DMG, AppImage/DEB/RPM/tarball | ⬜ |

## M0 Foundation ✅

- ✅ Workspace with enforced layers (engine crates build for the web target)
- ✅ Command registry: every action has an id, label, menu path, shortcut and params
- ✅ Command line `actcraft-cli`, local JSON control channel, MCP server with folder limits
- ✅ Show file `.actcraft`: versioned, older files load, unknown fields kept
- ✅ Gate `cargo xtask ci`: format, lints, tests, layers, web build, licences, assets
- ✅ Every command survives hostile input; every file parser has a fuzz test
- ⬜ CI workflow through storytold/craft-common-actions (own pull request)

## M1 Audio and library 🔶

- ✅ Real-time engine, tracks stay sample-locked (drift-free clock)
- ✅ 64 tracks per song: volume, pan, 4-band EQ, mute, name, colour, bus
- ✅ 16 output buses routed to interface outputs; choose the interface in Settings
- ✅ Six labelled mute groups; song and master volume
- ✅ Metronome: own bus, modes incl. visual-only, per-song count-in, start on the beat, latency offset
- ✅ Tempo: automatic detection, tap tempo, manual
- ✅ Autoplay, crossfade, fade-out, song loop; start/end time per song (by command)
- ✅ Waveform with zoom; click to jump
- ✅ Library search (boolean, fields, phrases, keywords), playlists with sets, smart playlists
- ✅ Queue (128), play history (500) with playlist from a date, double-click action
- ✅ Add songs from files (picker, drag and drop), names read with a pattern, duplicates found
- ✅ Settings window: interface, outputs, metronome, playback, library
- 🔶 Beat grid with snap (in the engine, no grid display yet)
- ⬜ MIDI file per song in sync (with M3)
- ⬜ Effect plugins per track and bus (format chosen after its licence check)
- ⬜ Export a song, playlist or archive; backup and restore
- ⬜ Drag-and-drop reorder in sets (buttons work today)

## M2 Lyrics, regions, sheet music 🔶 in progress

- ✅ Lyrics per song (Lyrics tab) with headings, bold/italic/underline and colours
- 🔶 Font tags (sans, mono work; serif and script wait for fonts from the shared font store)
- ✅ Chords in ChordPro syntax above the words; transpose, capo, hide chords; directives, chorus
- 🔶 Timecoded lines: the playing line is highlighted and scrolled to; smooth and constant-speed
  scrolling still to do
- 🔶 Lyrics editor: formatting buttons, chord and colour insert, time stamp at the playback
  position; search and replace and timecode-aware undo still to do
- 🔶 Import text, ChordPro and LRC files (done); lyrics in audio tags and DOCX text to do
- ✅ Up to 64 named, coloured regions per song: play, loop, skip, stop after; cues; jump live
  (Parts bar), leave a loop after its pass (L)
- 🔶 Markers from WAV files become regions (done); markers in other formats to do
- ⬜ PDF lyrics and sheet music with annotations, scaling, inverted colours, autoscroll
- ⬜ Second-screen window for lyrics and sheet music

## M3 MIDI and remote ⬜

- ⬜ MIDI in/out on hardware, virtual and network ports; MIDI file per song
- ⬜ Commands per song and at lyric timecodes; macros (`$CODE`); MIDI learn
- ⬜ MIDI Timecode and MIDI Clock out
- ⬜ Remote actions mapped to MIDI, keys, OSC and on-screen controls
- ⬜ Assignable on-screen buttons, knobs, sliders, toggles; clock modes; session timer

## M4–M6 Lighting ⬜

DMX output natively in Rust (Art-Net with node discovery, sACN with priorities, documented USB
interfaces), 16+ universes, fixture types and editor, Open Fixture Library import, patch, DMX
monitor; 2D stage preview with virtual beams; programming, presets, sequences and an effects
engine with 70+ templates; cues, cue lists, control pages, blind and freeze, tap tempo and beat
sync.

## M7–M9 ⬜

Lighting lane on the song timeline driven by the song's beat grid; band network (host/client,
remote mirroring); backup machine takeover; OSC, DMX input, joystick, Philips Hue; installers for
all platforms.

## Languages

- ✅ English built in and the default; every other language is a language file in
  `crates/actcraft-ui/i18n/` (German today). A new file adds a language with no code change
  (`docs/translating.md`). Settings > Language picks one, or the system language; remembered on
  this computer. A test fails when a language file lacks a window text.
- ✅ Status messages (show opened, songs added, tempo detected, audio errors...) in the window's
  language; the CLI and the control channel keep the English text
- ⬜ Command error texts (wrong or missing values) are still English
