# Roadmap

Milestones run in order. Each one ends green on `cargo xtask ci`, with
[docs/parity.md](docs/parity.md) (every feature with its command id and status) updated.

Legend: ✅ done · 🔶 partly done · ⬜ not started

| Milestone | What it brings | Status |
|---|---|---|
| M0 | Foundation: commands, CLI, control channel, MCP, build gate | ✅ |
| M1 | Audio engine, mixer, outputs, metronome, library, playlists, queue | 🔶 |
| M2 | Regions, lyrics and chords, sheet music (PDF, MusicXML notes and tab), lyrics editor, second screen | 🔶 in progress |
| M3 | MIDI in/out/sync, macros, remote actions, assignable on-screen controls | 🔶 in progress |
| M4 | DMX output (Art-Net, sACN, USB), fixtures and patch, fixture library, DMX monitor | ⬜ |
| M5 | 2D stage preview, programming, presets, sequences, effects engine | ⬜ |
| M6 | Cues, cue lists, control pages, blind and freeze, beat sync | ⬜ |
| M7 | One show: lighting lane on the song timeline, shared clock | ⬜ |
| M8 | Band network, backup machine, OSC, DMX input, joystick, Hue | ⬜ |
| M9 | Installers: MSI and zip, DMG, AppImage/DEB/RPM/tarball | ⬜ |

## Next up

The open items below, in the order they are planned. Each one is a pull request of its own;
details are in the milestone sections further down.

1. Lyrics from audio tags; markers from other formats than WAV (M2; needs readers in the shared
   audio library first, see below)
2. M3: OSC (needs an OSC
   reader: shared library or a dependency, see below)

## Quality and open questions

- ⬜ OSC (remote actions and, with M8, the band network) needs an OSC reader and writer; the
  shared libraries have none. Either one goes into `storytold/craft-libs` (house rule 16) or a
  crates.io crate is used; waiting on the owner's choice
- ⬜ Lyrics in audio tags (ID3 `USLT`, Vorbis comments, MP4 `©lyr`) and markers in formats other
  than WAV need readers that belong in the shared audio library (`craft-audio`, which reads WAV
  markers today), not in ActCraft (house rule 16); waiting on the owner's go-ahead to propose
  them there
- ⬜ Keyboard and accessibility-tree tests (kittest) for the window: the helpers don't exist in
  this repository yet; every new control should get one once they do
- ⬜ The MusicXML reader (`actcraft-notation`) is app-independent: offer it to the shared
  libraries (craft-libs), whose `craft-score` only writes MusicXML
- ⬜ Serif and script font tags wait for fonts from the shared font store
- ⬜ Effect plugins per track and bus: choose the plugin format after its licence check
- Waiting on the owner's confirmation: the logo's licence (MIT OR Apache-2.0, ATTRIBUTION.md) and
  the Apache-2.0 choice for `self_cell` (docs/licences.md)

## M0 Foundation ✅

- ✅ Workspace with enforced layers (engine crates build for the web target)
- ✅ Command registry: every action has an id, label, menu path, shortcut and params
- ✅ Command line `actcraft-cli`, local JSON control channel, MCP server with folder limits
- ✅ Show file `.actcraft`: versioned, older files load, unknown fields kept
- ✅ Gate `cargo xtask ci`: format, lints, tests, layers, web build, licences, assets
- ✅ Every command survives hostile input; every file parser has a fuzz test
- ✅ CI workflow through storytold/craft-common-actions on Linux, Windows and macOS

## M1 Audio and library 🔶

- ✅ Real-time engine, tracks stay sample-locked (drift-free clock)
- ✅ Transport: Play/Pause (Space) keeps the place; Stop (Esc) goes back to the song's start
- ✅ 64 tracks per song: volume, pan, 4-band EQ, mute, name, colour, bus
- ✅ 16 output buses routed to interface outputs; choose the interface in Settings
- ✅ Six labelled mute groups; song and master volume
- ✅ Metronome: own bus, modes incl. visual-only, per-song count-in, start on the beat, latency offset
- ✅ Tempo: automatic detection, tap tempo, manual
- ✅ Autoplay, crossfade, fade-out, song loop; start/end time per song (by command)
- ✅ Waveform with zoom; click to jump
- ✅ Waveforms kept in ActCraft's cache folder between runs (made again only when the audio file
  changes) and carried inside show packages; opening a show starts decoding its first song
- ✅ Library search (boolean, fields, phrases, keywords), playlists with sets, smart playlists
- ✅ Queue (128), play history (500) with playlist from a date, double-click action
- ✅ Add songs from files (picker, drag and drop), names read with a pattern, duplicates found
- ✅ Settings window: interface, outputs, metronome, playback, library
- ✅ Beat grid on the waveform (bars numbered, beats faint) from tempo and first beat; clicks snap
  to the nearest beat when "Snap to beat" is on
- ✅ MIDI file per song in sync: played to the MIDI output (Settings > MIDI) from the song's start,
  through its tempo map, moved by the MIDI latency offset; held notes released on stop and jumps
  (`midi.attach`, `midi.read`, `song.set {midi_file}`; Mixer tab: Attach MIDI file…)
- ⬜ Effect plugins per track and bus (format chosen after its licence check)
- ✅ Show package (`.actcraftpack`): the show and all its audio, MIDI, PDF and score files in one
  file, for backup or another computer; opening it unpacks the files beside it. A package can
  also hold one song or one playlist with its songs (Song and Playlist menus, the song's
  context menu, the playlist's Export package button; `song.export_package`,
  `playlist.export_package`, `show.export_package {song|playlist}`)
- ✅ Reorder songs in a set by dragging (a line shows where it lands) or with the Up/Down buttons

## M2 Lyrics, regions, sheet music 🔶 in progress

- ✅ Lyrics per song (Lyrics tab) with headings, bold/italic/underline and colours
- 🔶 Font tags (sans, mono work; serif and script wait for fonts from the shared font store)
- ✅ Chords in ChordPro syntax above the words; transpose, capo, hide chords; directives, chorus
- ✅ Timecoded lines: the playing line is highlighted; per song the lyrics jump to the line,
  glide smoothly between timecodes, scroll at constant speed over the song, or stay put
- ✅ Lyrics editor: formatting buttons, chord and colour insert, time stamp at the playback
  position, find and replace (also `lyrics.replace`); undo and redo (buttons, Ctrl+Z, Ctrl+Y)
  for typing and every button alike, so a time stamp or a replacement goes back as one step
- 🔶 Import text, ChordPro, LRC and DOCX files (done; DOCX through the shared `craft-docx`);
  lyrics in audio tags to do
- ✅ Up to 64 named, coloured regions per song: play, loop, skip, stop after; cues; jump live
  (Parts bar), leave a loop after its pass (L)
- 🔶 Markers from WAV files become regions (done); markers in other formats to do
- ✅ Spoken cues: the computer's own voice says the song title at the start and each region's
  words a set number of beats before it ("Chorus", "End 3 2 1"), on a bus of their own
  (Settings > Spoken cues); each phrase is spoken once and cached. A song's own Spoken cues tab
  adds cues at the playback position with one click (Verse, Chorus, End 3 2 1...) and plays from
  just before a cue to hear it
- ✅ PDF per song in the Sheet music tab: pages rendered in the background, zoom, inverted
  colours, scroll with playback; marks on the pages (pen, highlighter, notes, in four named
  colours) kept with the song and shown in the Stage window too (`pdf.mark`, `pdf.unmark`,
  `pdf.clear_marks`)
- ✅ Notation per song in the Sheet music tab: a MusicXML score (`.musicxml`, `.xml`,
  compressed `.mxl`, as notation and tablature editors export it) drawn as staves and tablature,
  part by part; frets come from the file or are placed on the part's tuning; eighths and shorter
  notes beamed by beat; the playing bar is marked on the song's beat grid, the view follows
  playback, a click on a bar plays from there (`notation.attach`, `notation.read`,
  `song.set {notation_file}`); drop a score on the window to attach it. Follow-ups:
  - ✅ Bar offset between score and audio: "Score bar 1 is song bar N" per song (0 or less for
    a pickup bar), used by the playing-bar mark, follow and click-to-bar
    (`song.set {score_first_bar}`)
  - ✅ Repeats and endings (first, second... endings), D.C., D.S., Fine, To Coda: drawn on the
    staff and tab, and the bars unrolled into the order they are played, so the playing bar,
    follow and click-to-bar are right on every pass (`notation.read` reports `played_bars`)
  - ✅ Ties and slurs drawn as arcs on the staff, over bar lines; one that continues on the next
    line runs to the line's edge (a tie as a short half tie); tied frets show as "(fret)" in
    the tab
  - ✅ Lyrics under the notes (`<lyric>`, every verse, hyphens between syllables), and chord
    symbols above the staff (`<harmony>`, as "Am7", "F#sus4/C#", "N.C.")
  - ✅ Techniques in the tab: bends (with their size and release), slides, hammer-ons and
    pull-offs, palm mute, harmonics, vibrato, tapping
  - ✅ Triplets and other tuplets marked with their number, and a bracket where the notes have
    no beam (or where the file asks for one)
  - ✅ Keyboard way to move to a bar: with the score focused, the arrow keys choose a bar (up and
    down by line), Home and End the first and last, Enter plays from it; the chosen bar is
    outlined and named for screen readers, as is the playing bar
  - ✅ Notation in the second window (large, follows playback), like the lyrics
  - Not planned: native files of tablature editors are not read, their formats are not publicly documented
    (clean room); they export MusicXML, which is read
- ✅ Second window (Stage window, opened from the Lyrics or the Sheet music tab): lyrics (large
  text, chords) or sheet music (the score at its own zoom, or the PDF at the window's width with
  inverted colours), full screen, follows playback; it keeps its own files, so it can show
  another song than the main window

## M3 MIDI and remote 🔶 in progress

- ✅ MIDI out and in on the computer's ports (the ports the system lists, hardware and virtual),
  chosen in Settings > MIDI and kept on the computer
- ✅ Macros: named lists of commands (Settings > Macros; `macro.set`, `macro.remove`, `macro.list`,
  `macro.run`); `$NAME` at a timecoded lyrics line runs macro NAME when playback reaches the line
  (and is not shown in the lyrics); a jump over a line does not run it
- ✅ MIDI Clock (with start, stop, continue and song position) and MIDI Timecode (quarter frames,
  a full frame on a jump; 24, 25 or 30 fps) out to a MIDI output chosen in Settings > MIDI
  (`midi.set`); the output is kept on the computer, what is sent with the show
- 🔶 Remote actions: a MIDI note, controller (a foot switch, once per press) or program change,
  on one channel or any, runs a command that needs nothing more; set up in Settings > MIDI by
  hand or with MIDI learn (`remote.add`, `remote.remove`, `remote.list`, `remote.trigger`).
  Keys have their shortcuts; pads on the screen (below); OSC to do
- ✅ Pads: buttons along the bottom of the window that run a command or a macro with one press,
  in a named colour (`pad.add`, `pad.remove`, `pad.list`, `pad.press`; Edit pads), and faders
  for the master, the song or a bus volume (`pad.add {fader}`), and toggles that switch the click, looping the song or playing
  the next song and show whether they are on (`pad.add {toggle}`)
- ✅ Clock modes (click the big clock): elapsed, remaining, until the queue ends, time of day and
  show time (from the first Play, pauses included; its menu starts it again)

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
- ✅ Command errors show in the window without their kind and translated, also those that carry
  a value (a song id, a limit, a file name): the window finds the English template the text was
  made from (`error_text::TEMPLATES`, checked against every such error in the source) and
  translates it around the value
