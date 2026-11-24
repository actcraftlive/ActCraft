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
| M9 | Installers: MSI and zip, DMG, AppImage/DEB/RPM/tarball | 🔶 unsigned zip and tarball (`cargo xtask package`) and the update check (Help > About, only when asked) done; installers, CI artifacts and a first alpha release to do |
| M10 | Band sheets: sheets per musician, a browser view for musicians, MIDI to notes, setlist PDF | ✅ sheets per musician, setlist PDF, MIDI to notes, sheets in the browser |
| M11 | Light sequences: animations for moving heads, steps on the click, started by cues | 🔶 sequences done; more from the owner's list to come |
| M12 | Help: a manual with how-tos and screenshots in the program, in every language, opened from the settings it explains | ✅ done |

## Next up

The open items below, in the order they are planned. Each one is a pull request of its own;
details are in the milestone sections further down.

1. Lyrics from audio tags; markers from other formats than WAV (M2; the readers are proposed to
   the shared audio library first, see below)
2. M9: unsigned installers (MSI, DMG, AppImage/DEB/RPM) and CI build artifacts (owner agreed; no release, no signing; a portable zip or tarball is `cargo xtask package`)

M10 (band sheets, planned with the owner on 2026-11-23) is done; see its section.

Next, asked for by the owner on 2026-11-24 (details under M12):

3. ✅ M12: help window and file layout
4. ✅ M12: the help pages in English and German
5. ✅ M12: screenshots in each language (`cargo xtask help-shots`)
6. ✅ M12: question-mark buttons that open the help at their section
7. ✅ M12: translating the help (`docs/translating.md`)

Asked for by the owner on 2026-11-24 (details under M11):

8. ✅ M11: Stop for cue lists and All stop
9. ✅ M11: one cue in control (a new cue takes over from the one before)
10. ✅ M11: hits, short bursts outside the cue lists (fog for 0.5 s)
11. ✅ M11: presets keep every channel of a fixture

Asked for by the owner on 2026-11-24, so people moving from other lighting programs keep what
they have (details under M3 and M4–M6):

13. ⬜ M3: remote actions with a target: a MIDI note, controller or program change recalls a
    chosen preset, fires a hit, goes to a cue in a cue list, starts a sequence or runs a macro

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
  (Parts bar), leave a loop after its pass (L; also Transport > Leave Loop, so a MIDI foot switch,
  a pad, a game controller or OSC can end the loop: the song plays the loop to its end, then goes
  on)
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

## M3 MIDI and remote ✅

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
- ⬜ Remote actions with a target, so the MIDI a show already sends (from a backing-track
  program, a keyboard, a foot controller) keeps working after moving from another lighting
  program: the action names what it acts on (a preset, a hit, a cue list or one of its cues, a
  sequence, a macro) and finds it by name when the message comes in (`remote.add {target, cue}`)
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

## M4–M6 Lighting ✅

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
- ✅ Named ranges on fixture channels (one channel doing several things: off, dimmer, strobe):
  set by picking a range and a level in the Fixtures window (`fixture.range`), kept by fixture
  types and set in the fixture type editor (Ranges beside each channel: from, to, name); `.r20`
  fixture files can be imported (`fixture_type.import`) and their ranges added there
- ✅ Fixture library: ActCraft brings fixture types of its own (first the Betopper LM70SR, 14 and
  9 channels, with every range from its manual), offered under Kind in the Fixtures window and
  named in the window's language; a type can be exported as an ActCraft fixture file (.json) and
  imported into another show or on another computer (`fixture_library.list`,
  `fixture_type.from_library`, `fixture_type.export`, `fixture_type.import`)

DMX output natively in Rust (Art-Net with node discovery, sACN with priorities, documented USB
interfaces), 16+ universes, fixture types and editor, Open Fixture Library import, patch, DMX
monitor; 2D stage preview with virtual beams; programming, presets, sequences and an effects
engine with 70+ templates; cues, cue lists, control pages, blind and freeze, tap tempo and beat
sync.

## M7–M9 🔶 (M9 installers to do)

- ✅ Lighting lane: the Light cues tab shows the song as a lane with its bars, where playback is
  and each lighting cue as a mark; a click on the lane sets where a new cue goes (on the beat with
  Snap to beat), a click on a mark jumps there, dragging a mark moves its cue, and the list moves
  a cue a beat earlier or later and says its bar and beat. Cues are placed by time or by bar and
  beat (`song.light_cue_add {bar, beat}`, `song.light_cue_move`)
- ✅ DMX in: Settings > Lighting listens for Art-Net and sACN (sACN universe 1 is Art-Net 0) from a desk or another program (this
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

## M10 Band sheets ✅

Planned with the owner on 2026-11-23 and done the same day, one pull request per item.

- ✅ Several sheets per song, each for a musician: besides its main sheet a song holds up to 32
  further sheets, each with a name ("Guitar", "Bass"), the musician it is for (or anyone), a PDF
  and/or a MusicXML score (and which part of it), its own marks and its own first bar; the Sheet
  music tab picks the sheet shown (the second screen follows), adds, names and removes sheets and
  keeps the band's musicians. Older shows load unchanged: their PDF and score are the main
  sheet. A musician sees their own sheet, else one for anyone (`musician.add`, `sheet.add`,
  `sheet.set`, `sheet.for`, …; `pdf.mark` and `notation.attach` take `sheet`)
- ✅ Musicians follow their sheets in a browser (Settings > Band > Sheets in the browser, off until
  switched on): ActCraft serves pages on the local network only; a musician opens the address
  shown on a tablet or laptop, signs in once with a six-digit code, picks their name and sees
  the sheet made for them (else one for anyone) for the song that plays: a PDF sheet as pictures
  of the page in view and the next, turning as the song goes on; it says so when there is none.
  Plain HTML without scripts (house rule 4): a page asks for itself again and the answer waits
  until there is something new. A score sheet (MusicXML or MIDI) shows as a picture of the three
  lines from the one playing, drawn as the Sheet music tab draws it (notes and tab), moving on
  line by line
- ✅ Notes from MIDI files: a MIDI file (.mid) is read as a score like MusicXML ("Attach notation…",
  or "Notes from the MIDI file" for the song's own MIDI file); each track, and each channel of a
  track, becomes a part a sheet can show for its musician. Onsets and ends go to a sixteenth grid,
  notes struck together form chords, odd lengths and bar lines are tied, sharps or flats follow
  the key; drums are written by the General MIDI drum map, a piano on two staves split at middle
  C, guitars and basses (by program or name) also as tab. Works best with MIDI written in a
  notation program, roughly with played-in MIDI; voices within one staff are not separated
- ✅ Setlist as a PDF for the stage floor (Playlist > Print Setlist as PDF…): the open playlist on
  A4 with the band's logo (Playlist > Band Logo…) and the concert's name (the playlist's) at the
  top, then each set with its time, each song's number, title and length, and the time of all;
  as large as fits one page, smaller and onto more pages only for long lists. A line can be set
  before any song of a set, such as a break or an instrument change (Note beside the song;
  `set.note`). Written with the shared `craft-pdf` writer in the window's own font; desktop only
  (`playlist.export_setlist`, `setlist.logo`)
- ✅ Looping a part (asked for with this plan; already there): a region in loop mode repeats, and
  Leave Loop (L, or a MIDI foot switch through a remote action) lets the current pass play to its
  end and goes on

## M11 Light sequences 🔶

Asked for by the owner on 2026-11-24 ("whole runs as a cue, so moving heads can be animated";
"single beats of the click can be different things").

- ✅ Sequences (Presets window > Sequences): steps of looks over time, each a preset the lights
  move to over part of the step and hold for the rest; they run over and over, once, or back and
  forth. Step lengths are seconds, or beats of the song's click (each beat or bar can be another
  look, on the song's tempo and bar). Record step stores the look on stage as the next step;
  steps can be given another look, length and move time, moved and removed. A cue can start a
  sequence or an effect, or stop all of them, with or without a preset of its own, so one cue
  (from the song's lighting lane, a pad, MIDI or a foot switch) runs a whole animation
  (`sequence.set`, `sequence.step_add` and the other `sequence.*` commands, `cue.set {starts,
  stops}`, `sequence.toggle` for pads)
- ✅ Stop a cue list (owner, 2026-11-24: "started cues can't be stopped, the lights stay on, where is
  the stop button?"): a Stop button beside Go and Back, and a command for pads, MIDI and the menu.
  It puts the list back to "not started" (no more going on by itself), stops the sequences and
  effects its cues started, and darkens the fixtures its presets light, without moving them
  (`cue_list.stop`)
- ✅ All stop (owner, 2026-11-24: "and a general ALL STOP button"): one button in the Presets
  window, the Lighting menu and a command for pads, MIDI and foot switches that stops every cue
  list, sequence and effect and darkens every fixture without moving it (`lighting.stop_all`)
- ✅ One cue in control (owner, 2026-11-24: "light cues in a song overwrite each other from the
  second one on: it doesn't stop the cue after its part, both fight for the lights"): a cue that
  goes takes over. Every other cue list stops going on by itself, and the sequences and effects
  the cues before it started stop, unless this cue starts them too (they then go on without
  starting over)
- ✅ Hits (owner, 2026-11-24: "single things I can put in, e.g. fire a fog machine for 0.5 s, not
  cues but small bursts outside the cue lists"): a hit is a name, a preset and a length. Firing it
  puts the preset on top for that long, then puts back exactly what those channels were before.
  Hits belong to no cue list, so a cue taking over leaves them alone. Fired from the song's
  lighting lane at a bar, from the Presets window and by a command (`hit.*`, `song.light_cue_add
  {hit}`); pads and foot switches next
- ✅ Presets keep every channel (owner, 2026-11-24: "why are only 7 values of my 13-value DMX
  profile stored? Switching the light on is not in my preset"): channels without a named
  attribute (the LM70SR's dimmer/strobe, colour macro, speed, reset) were stored once per
  fixture, so only the first one was kept. Each such channel is now stored by its number, and
  recall, fades, sequences and hits set it

## M12 Help and how-tos ✅

Asked for by the owner on 2026-11-24: a help with screenshots, important information and how-tos;
question-mark buttons beside settings that lead to their part of the help; the help part of the
language files, so it can be translated, with each language's screenshots in its own folder.
Each step is its own pull request, in this order:

1. ✅ Help window and file layout: Help > ActCraft Help (F1) shows pages written in Markdown in the
   language folder (`crates/actcraft-ui/i18n/help/<language>/*.md`), screenshots from
   `crates/actcraft-ui/i18n/help/img_<language>/`; a page or picture missing in a language shows
   the English one. A contents list, links between pages and to sections, back and forward,
   all by keyboard and with names for screen readers. The Markdown is read with the shared
   `craft-markdown` (house rule 16); the window draws it.
2. ✅ The pages, in English and German: getting started; songs, tracks and the mixer; playlists,
   setlists and the printed setlist; lyrics and chords; sheet music, sheets per musician and the
   browser view; MIDI, foot switches, pads and OSC; lighting: DMX output, the USB cable,
   fixtures and the fixture library; presets, cues, effects and sequences; the band link;
   updates; keyboard shortcuts.
3. ✅ Screenshots in each language, made from the program itself by `cargo xtask help-shots`
   (`--screenshot` with a sample show), so they are made again after a change and for a new
   language; recorded in ATTRIBUTION.md.
4. ✅ Question-mark buttons beside settings and in windows that open the help at their section.
5. ✅ `docs/translating.md` explains how to translate the help and its screenshots.

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
