# Roadmap

Milestones run in order. Each one ends green on `cargo xtask ci`, with
[docs/parity.md](docs/parity.md) (every feature with its command id and status) updated.

Legend: ✅ done · 🔶 partly done · ⬜ not started

| Milestone | What it brings | Status |
|---|---|---|
| M0 | Foundation: commands, CLI, control channel, MCP, build gate | ✅ |
| M1 | Audio engine, mixer, outputs, metronome, library, playlists, queue | 🔶 |
| M2 | Regions, lyrics and chords, sheet music (PDF, MusicXML notes and tab), lyrics editor, second screen | 🔶 in progress |
| M3 | MIDI in/out/sync, macros, remote actions, assignable on-screen controls | ✅ |
| M4 | DMX output (Art-Net, sACN, USB), fixtures and patch, fixture library, DMX monitor | ✅ |
| M5 | 2D stage preview, programming, presets, sequences, effects engine | ✅ |
| M6 | Cues, cue lists, control pages, blind and freeze, beat sync | ✅ |
| M7 | One show: lighting lane on the song timeline, shared clock | ✅ lighting lane; light cues, beat effects and MIDI clock all follow the song |
| M8 | Band network, backup machine, OSC, DMX input, joystick, Hue | ✅ band link with pairing code (a follower keeps playing when the host goes), OSC in, DMX in with triggers, game controllers, Hue |
| M9 | Installers: MSI and zip, DMG, AppImage/DEB/RPM/tarball | 🔶 unsigned zip and tarball (`cargo xtask package`) done; installers and CI artifacts to do |

## Next up

The open items below, in the order they are planned. Each one is a pull request of its own;
details are in the milestone sections further down.

1. Lyrics from audio tags; markers from other formats than WAV (M2; the readers are proposed to
   the shared audio library first, see below)
2. M9: unsigned installers (MSI, DMG, AppImage/DEB/RPM) and CI build artifacts (owner agreed; no release, no signing; a portable zip or tarball is `cargo xtask package`)

## Quality and open questions

- ✅ OSC: the shared libraries have none; the owner chose a crates.io crate (`rosc`,
  MIT OR Apache-2.0), and the same for DMX (Art-Net, sACN)
- ⬜ Lyrics in audio tags (ID3 `USLT`, Vorbis comments, MP4 `©lyr`) and markers in formats other
  than WAV need readers that belong in the shared audio library (`craft-audio`, which reads WAV
  markers today), not in ActCraft (house rule 16); the owner agreed to propose them there
- 🔶 Keyboard and accessibility-tree tests (kittest): `crates/actcraft-ui/src/ui_tests.rs` drives the
  window through its AccessKit tree with the accessibility check on (pads, DMX monitor, cue lists,
  presets); older controls get tests as they are touched
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
- ✅ Remote actions: a MIDI note, controller (a foot switch, once per press) or program change,
  on one channel or any, runs a command that needs nothing more; set up in Settings > MIDI by
  hand or with MIDI learn (`remote.add`, `remote.remove`, `remote.list`, `remote.trigger`).
  Keys have their shortcuts; pads on the screen (below). OSC: Settings > MIDI listens on a UDP
  port (this computer only, or the network too); `/actcraft/<command id>`, `/actcraft/macro/<NAME>`
  and `/actcraft/pad/<n>` run a command, a macro or a pad (`osc.trigger`)
- ✅ Pads: buttons along the bottom of the window that run a command or a macro with one press,
  in a named colour (`pad.add`, `pad.remove`, `pad.list`, `pad.press`; Edit pads); faders for
  the master, the song or a bus volume (`pad.add {fader}`); toggles that switch the click,
  looping the song or playing the next song and show whether they are on (`pad.add {toggle}`)
- ✅ Pad pages (control pages): pads sit on up to eight pages and the bar shows one at a time;
  Page before and Next page in the bar and in the Pads menu (so keys, MIDI and OSC turn pages),
  a pad moved to another page while editing; OSC pad numbers count on the page shown
  (`pad.page`, `pad.page_next`, `pad.page_before`, `pad.move`, `pad.add {page}`)
- ✅ Clock modes (click the big clock): elapsed, remaining, until the queue ends, time of day and
  show time (from the first Play, pauses included; its menu starts it again)

## M4–M6 Lighting 🔶

- ✅ DMX levels: 64 universes of 512 channels, live (not saved), set by command (`dmx.set`,
  `dmx.clear`, `dmx.get`); Lighting > DMX monitor shows a universe, a channel is set by number
  or by clicking it
- ✅ DMX output: Settings > Lighting sends a universe over Art-Net (a port address, broadcast or
  to one address) or sACN (a universe and priority, multicast or to one address), when it
  changes and again every 0.8 s (`dmx.output_add`, `dmx.output_remove`, `dmx.output_list`).
  "Find Art-Net nodes" asks the network (ArtPoll) and lists who answered with their universes;
  one click uses a node for the next output. A USB interface that speaks the DMX USB Pro serial
  protocol sends a universe on a serial port picked from the ones the computer has ("Find USB
  interfaces"; `dmx.output_add {protocol: usb_pro, address: port}`); a plain USB-to-DMX cable (an FTDI
  chip, no processor of its own) works too: ActCraft makes the DMX signal itself (break, 250 000
  baud 8N2, about 20 frames a second; `protocol: usb_cable`), tried with a moving head
- ✅ Fixtures and patch: Lighting > Fixtures patches lights by kind (dimmer, RGB, RGBW, RGBAW+UV,
  a moving head with 16-bit pan and tilt) or channel by channel into a universe, one or a row of
  them, at an address or the first free one, refusing overlaps; each fixture's attributes are set
  with sliders (`fixture.add`, `fixture.move`, `fixture.remove`, `fixture.list`, `fixture.kinds`,
  `fixture.set`)
- ✅ Open Fixture Library import: "Import fixture type…" reads a fixture file (.json) into the show (and since #89
  `.r20` fixture files)
  as a type with its modes; each channel's capability becomes its attribute and fine channels
  follow theirs; patched by type and mode (`fixture_type.import`, `fixture_type.list`,
  `fixture_type.remove`, `fixture.add {type, mode}`)
- ✅ Fixture type editor: a new type, or a change to the chosen one, with its modes and each
  channel's attribute, moved up and down or removed (`fixture_type.set`)
- ✅ Groups and presets: fixtures picked together by name (`group.set`), set as one
  (`fixture.set {group}`); presets keep what fixtures show (every fixture or a group) and bring
  it back with one press (`preset.store`, `preset.recall`); Lighting > Presets. Fixtures have ids
  so groups and presets follow them
- ✅ Fades and cue lists: a preset fades in over seconds (`preset.recall {fade}`); cue lists go
  through presets with Go and Back (`cue.go`, `cue.back`, `cue.jump`; Lighting > Next Cue and Cue
  Before, so pads, keys, MIDI and OSC can run them); a cue can go on to the next by itself after
  some seconds, and after the last back to the first, so a cue list runs as a sequence or chase
  (`cue.add {follow}`, "Then on after")
- ✅ Light cues per song: a song fires cues of a cue list at its times as it plays, like its
  macros (a jump skips what it passes; `song.light_cue_add`, `song.light_cue_remove`; the song's
  Light cues tab with "Now")
- ✅ Effects: a group's attribute moved in a wave (smooth, on and off, ramp, up and down) between two
  values, seconds or beats per cycle at the song tempo, spread over the group; started and
  stopped by name (`effect.set`, `effect.start`, `effect.stop`; Lighting > Stop Effects); one
  effect moves more attributes on the same wave, each later in the cycle (pan and tilt make a
  circle, red, green and blue a rainbow; `effect.set {also}`, "Move another attribute too"). An
  effect counted in beats locks to the playing song's beat grid, so its pulses land on the beat
  wherever it was started
- ✅ Ready-made effects: seventy to start from (breathe, pulses and waves on the beat or over a
  bar, chases, builds, bounce, every other light, colour pulses, drifts and chases, red and blue,
  warm and cool, embers, ocean, sweeps, fans, nods, circles, rainbows, zooms); none flashes
  more than three times a second (`effect.templates`, `effect.from_template`; "Start from" in the
  effect form)
- ✅ Blackout, master and freeze: blackout takes every fixture dark at the output and a master
  scales how bright they are, by their intensity (or colour) channels only, so positions and the
  stored levels stay (`dmx.blackout`, `dmx.master`); freeze holds fades and effects and lets them
  go on from there (`lighting.freeze`). In the Presets window, the Lighting menu and as toggle
  pads
- ✅ Blind: fixture changes and recalled presets go to a copy of the levels, not to the lights, so
  looks are built and stored as presets while the show goes on; cues, fades and effects keep
  playing live, and the Fixtures and Stage windows say blind is on (`lighting.blind`; Presets
  window, Lighting menu, toggle pads)
- ✅ 2D stage preview: Lighting > Stage shows the fixtures from above in the colour they show now;
  placed by dragging or by numbers (`fixture.place`; `fixture.stage` lists places and colours).
  Light shows under each fixture: a glow where it shines down, and for a moving light a beam out
  to where pan and tilt send it, as wide as its zoom and as bright as the light; its name says
  where it points (`fixture.stage` gives `beam`). Pan 540° and tilt 270° are taken for every
  moving light until fixture types carry their ranges

DMX output natively in Rust (Art-Net with node discovery, sACN with priorities, documented USB
interfaces), 16+ universes, fixture types and editor, Open Fixture Library import, patch, DMX
monitor; 2D stage preview with virtual beams; programming, presets, sequences and an effects
engine with 70+ templates; cues, cue lists, control pages, blind and freeze, tap tempo and beat
sync.

## M7–M9 🔶

- ✅ Lighting lane: the Light cues tab shows the song as a lane with its bars, where playback is
  and each lighting cue as a mark; a click on the lane sets where a new cue goes (on the beat with
  Snap to beat), a click on a mark jumps there, dragging a mark moves its cue, and the list moves
  a cue a beat earlier or later and says its bar and beat. Cues are placed by time or by bar and
  beat (`song.light_cue_add {bar, beat}`, `song.light_cue_move`)
- ✅ DMX in: Settings > Lighting listens for Art-Net from a desk or another program (this
  computer only, or other computers too); the DMX monitor shows each universe heard, read only
  (`dmx.input_list`, `dmx.input_get`); a channel heard there runs a command when it goes up past
  half, once, like a MIDI remote action (`dmx.trigger_add`, `dmx.trigger_remove`,
  `dmx.trigger_list`; "Add trigger")
- ✅ Game controllers: a button on any connected controller runs a command, like a MIDI remote
  action; Settings > MIDI lists the connected controllers, learns a button by pressing it and
  saves the buttons with the show (`gamepad.add`, `gamepad.remove`, `gamepad.list`,
  `gamepad.press`); a stick or an analogue trigger moves a volume (master, song, a bus) or the
  lighting master (`gamepad.fader_add`, `gamepad.move`; "Add fader")
- ✅ Philips Hue, first part: Settings > Lighting finds a bridge on this network (mDNS, no online
  service) and pairs with it once its round button is pressed; the key stays in this computer's
  settings, never in the show; the bridge's lights are listed. A Hue light can follow a fixture
  (Fixtures window, `fixture.hue`): it shows the colour and brightness the fixture sends, blackout
  and master included, sent only when it changes, about nine changes a second in all
- ✅ Band link: Settings > Band lets other ActCraft computers on the local network follow this one
  (off until switched on); a follower pairs once with a six-digit code and keeps a token; it
  plays the host's song (by title, from its own copy), plays and stops with it and jumps to its
  place when more than half a second off. If the host stops answering the follower plays on and
  connects again every few seconds (the backup machine). Only local network addresses are let in;
  five wrong codes change the code

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
