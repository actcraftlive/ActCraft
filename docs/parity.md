# Feature parity

Every requested feature has an id, the command(s) that drive it and a status:
**done** (works and tested), **partial** (works with stated gaps), **planned** (milestone shown).
Statuses describe what ran in tests, not what is designed.

## M0 Foundations

| Id | Feature | Command ids | Status |
|---|---|---|---|
| M0.1 | Workspace and enforced layers | `cargo xtask layers` | done |
| M0.2 | Command registry (id, label, menu, shortcut, params doc, enabled, run, panic guard) | `app.commands` | done |
| M0.3 | CLI | `actcraft-cli commands/run/send/state/serve` | done |
| M0.4 | JSON control channel on localhost | `commands.list`, `command.run`, `state.get` | done |
| M0.5 | MCP server with automation roots | `actcraft-cli mcp` | done |
| M0.6 | Gate: fmt, clippy, tests, layers, wasm, licences, assets | `cargo xtask ci` | done (wasm step needs the target installed) |
| M0.7 | Hostile-params sweep over every command | test `every_command_survives_hostile_params` | done |
| M0.8 | Show file `.actcraft`: versioned, old files load, unknown fields kept | `show.new/open/save/info` | done |

## A1 Songs and audio

| Id | Feature | Command ids | Status |
|---|---|---|---|
| A1.1 | ≥32 tracks per song (64) with volume, pan, 4-band EQ, mute, name, colour, bus | `track.add/remove/set/set_eq` | done |
| A1.2 | 16 output buses, mono-to-stereo routing, song and master volume | `bus.set`, `mixer.song_volume`, `mixer.master_volume` | done |
| A1.3 | Realtime engine, no allocation or locks in the callback | renderer | done |
| A1.4 | Continuous drift correction between tracks | renderer (60 s timing test) | done |
| A1.5 | Output-latency compensation for click | `metronome.set {latency_offset_ms}`, `midi.set {latency_offset_ms}` | done: the click's offset, and the MIDI file's (Settings > MIDI) |
| A1.6 | Start/end time per song | `song.set {start, end}` | done |
| A1.7 | Effect plugin hosting | - | planned (format choice pending licence check) |
| A1.8 | 6 mute groups with labels and remote actions | `mute_group.toggle/set`, `track.mute_group` | done |
| A1.9 | Edit track name and colour from the mixer | `track.set` | done (name field and colour picker on each strip) |
| A1.10 | Metronome on own bus; modes incl. visual-only | `metronome.toggle/cycle_mode/set` | done |
| A1.11 | Per-song count-in | `song.set {count_in_bars}`, `metronome.count_in` | done |
| A1.12 | Start quantized to the beat | `metronome.set {quantized_start}` | done |
| A1.13 | Beat anchor independent of song start; BPM per song; grid with snap | `song.set {beat_anchor, bpm}` | done: grid drawn on the waveform, click-to-jump snaps to beats |
| A1.14 | MIDI file per song in sync | `midi.attach`, `midi.read`, `song.set {midi_file}` | done: played to the MIDI output from the song's start through its tempo map, with the MIDI latency offset |
| A1.15 | Autoplay, crossfade (ignores requests while running), fade-out, song loop | `song.set {autoplay, fade_out, loop_song}`, `transport.crossfade_to`, `transport.set_crossfade` | done |
| A1.16 | Waveform view with zoom | `waveform.peaks` | done |
| A1.17 | Tempo detection on import, tap tempo | `song.detect_bpm`, `song.set {bpm}` | done |
| A1.18 | Bus-to-output routing and interface choice in Settings | `bus.set {output}` (interface: window only) | done |
| A1.19 | Audio decodes in the background; playback starts when ready | engine loader | done |

## A4 Library, setlists, queue

| Id | Feature | Command ids | Status |
|---|---|---|---|
| A4.1 | Metadata (title, artist, key, BPM, year, duration, case-distinct keywords, capo, transpose) | `song.add/set/list` | done |
| A4.2 | Search: AND/OR/NOT, -term, title:/artist:, quoted phrases, keyword filter | `library.search` | done |
| A4.3 | Playlists split into sets | `playlist.create/add_set/add_songs` | done |
| A4.4 | Smart playlists (match all/any) | `playlist.create_smart` | done |
| A4.5 | Non-destructive quick sort per set | `playlist.sorted_view` | done (command; no UI yet) |
| A4.6 | Multi-select reorder | `playlist.move_songs` | done (pick songs in a set, then move them up or down together or remove them; one song by dragging or Up/Down) |
| A4.7 | Confirmation before delete | `playlist.delete {confirm}`, `song.remove {confirm}` | done |
| A4.8 | Queue up to 128, add set, play next, add to queue | `queue.*` | done |
| A4.9 | Configurable double-tap | `queue.set_double_tap`, `queue.double_tap` | done |
| A4.10 | History of 500 with done marker; playlist from a date | `history.list/mark_done/playlist_from_date` | done |
| A4.11 | Songs from audio files, filename format, duplicates, alphabetical import | `song.import_files`, `song.duplicates` | done |
| A4.12 | Export song/playlist/archive, backup and restore | `show.export_package`, `song.export_package`, `playlist.export_package`, `show.open_package` | done: the whole show, one song or one playlist with its songs, with their files, as one package (backup, move, hand over) |

## Later milestones

| Area | Status |
|---|---|
| A2 Regions | done: 64 per song, play/loop/skip/stop after, cues, live jump, leave loop, WAV markers (`region.*`); markers from other formats open |
| Spoken cues (requested by the band) | done: title at the song start and region words before each region with the computer's own speech (Windows System.Speech, macOS `say`, Linux espeak-ng/pico2wave), own bus and volume (`voice.set`, `voice.prepare`, `song.set {announce_title}`, `region.set {voice}`) |
| A3 Lyrics, chords, PDF, editor, second screen | partial: lyrics with markup and ChordPro chords (`song.set {lyrics}`), transpose/capo, playing-line highlight, scrolling per song (jump to the line, smooth, constant speed, off: `song.set {lyrics_scroll}`), editor with formatting, time stamps (`lyrics.stamp_line`), find and replace (`lyrics.replace`) and undo/redo over typing and buttons alike, import of text/ChordPro/LRC/DOCX (`lyrics.import`); second window for the stage with large lyrics or sheet music (Stage window in the Lyrics and Sheet music tabs); PDF per song in the Sheet music tab (`song.set {pdf_file}`) with zoom, inverted colours and scrolling with playback, and marks on the pages: pen, highlighter, notes (`pdf.mark`, `pdf.unmark`, `pdf.clear_marks`); MusicXML score per song as staves and tablature with the playing bar marked, its bar 1 placed on any song bar, repeats, endings and D.C./D.S. jumps played in order, ties and slurs as arcs, tuplets with their number, playing techniques in the tab, lyrics under the notes and chord symbols above them (`notation.attach`, `notation.read`, `song.set {notation_file, score_first_bar}`); several sheets per song, each for a musician (`sheet.add`, `musician.add`), MIDI files read as notes and tab (`notation.attach` with a `.mid`), each musician's sheet in a browser on the local network (Settings > Band), a printed setlist (`playlist.export_setlist`); open: lyrics from audio tags, serif and script fonts |
| A5 MIDI, macros, sync, remote actions | partial: MIDI Clock and MIDI Timecode out to a chosen MIDI output (`midi.set`); a MIDI file per song (`midi.attach`); remote actions from a MIDI input with MIDI learn (`remote.add`, `remote.trigger`); macros run from lyrics lines (`$NAME`) or at once (`macro.set`, `macro.run`); pads on the screen (`pad.add`, `pad.press`); OSC messages run commands, macros and pads (`osc.trigger`) |
| A6 Stage UI | partial: sidebar hides during playback, clock modes, audio indicator, status messages, on-screen pads with pages, a second window for the stage (lyrics or sheet music) and the 2D stage view; nothing further planned yet |
| A7 Band network | done: band link with a pairing code on the local network; a follower plays the host's song from its own copy, plays and stops with it and plays on when the host goes (Settings > Band) |
| B1-B8 Lighting | partial: DMX levels in 64 universes, a DMX monitor (`dmx.set`, `dmx.get`), output over Art-Net and sACN with node search (`dmx.output_add`), fixtures and the patch (`fixture.add`, `fixture.set`), Open Fixture Library import and a fixture type editor (`fixture_type.import`, `fixture_type.set`), groups and presets (`group.set`, `preset.store`, `preset.recall`), fades and cue lists (`cue.go`), light cues per song (`song.light_cue_add`), effects with ready-made templates (`effect.set`, `effect.from_template`), a 2D stage preview (`fixture.place`), blackout, master, freeze and blind (`dmx.blackout`, `dmx.master`, `lighting.freeze`, `lighting.blind`), pad pages (`pad.page`); also beat-locked effects, DMX in over Art-Net and sACN with triggers (`dmx.input_get`, `dmx.trigger_add`), game controller buttons and sticks (`gamepad.add`, `gamepad.fader_add`), Philips Hue lights following fixtures (`fixture.hue`), USB interfaces and plain USB-DMX cables (`dmx.output_add {protocol: usb_pro | usb_cable}`), beams in the stage preview, a lighting lane on the song timeline (`song.light_cue_add {bar}`) and effects over several attributes (`effect.set {also}`), named ranges on mixed channels (`fixture.range`), `.r20` fixture files and `.lightkeyfxt` fixture profiles (`fixture_type.import`) |
| C Merge (lighting lane, shared clock) | done: a lighting lane on the song timeline (`song.light_cue_add {bar}`), light cues and beat-locked effects on the song's clock |
| D Replacements (CLI, control channel, MCP) | done for automation; rest with their features |
| Languages EN and DE | done: English built in (default), other languages are language files (`crates/actcraft-ui/i18n/<code>.tsv`, German shipped, completeness checked by a test); status messages translated through templates; command errors translated, those with values through templates (`error_text.rs`) |
