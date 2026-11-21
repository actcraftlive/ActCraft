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
| A1.5 | Output-latency compensation for click | `metronome.set {latency_offset_ms}` | partial (MIDI file side waits for A5) |
| A1.6 | Start/end time per song | `song.set {start, end}` | done |
| A1.7 | Effect plugin hosting | - | planned (format choice pending licence check) |
| A1.8 | 6 mute groups with labels and remote actions | `mute_group.toggle/set`, `track.mute_group` | done |
| A1.9 | Edit track name and colour from the mixer | `track.set` | partial (name in mixer; colour by command) |
| A1.10 | Metronome on own bus; modes incl. visual-only | `metronome.toggle/cycle_mode/set` | done |
| A1.11 | Per-song count-in | `song.set {count_in_bars}`, `metronome.count_in` | done |
| A1.12 | Start quantized to the beat | `metronome.set {quantized_start}` | done |
| A1.13 | Beat anchor independent of song start; BPM per song; grid with snap | `song.set {beat_anchor, bpm}` | partial (snap in engine clock, no grid UI yet) |
| A1.14 | MIDI file per song in sync | `song.set {midi_file}` | planned M3 |
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
| A4.6 | Multi-select reorder | `playlist.move_songs` | partial (command takes many; UI moves one at a time, no drag yet) |
| A4.7 | Confirmation before delete | `playlist.delete {confirm}`, `song.remove {confirm}` | done |
| A4.8 | Queue up to 128, add set, play next, add to queue | `queue.*` | done |
| A4.9 | Configurable double-tap | `queue.set_double_tap`, `queue.double_tap` | done |
| A4.10 | History of 500 with done marker; playlist from a date | `history.list/mark_done/playlist_from_date` | done |
| A4.11 | Songs from audio files, filename format, duplicates, alphabetical import | `song.import_files`, `song.duplicates` | done |
| A4.12 | Export song/playlist/archive, backup and restore | - | planned M1 |

## Later milestones

| Area | Status |
|---|---|
| A2 Regions | done: 64 per song, play/loop/skip/stop after, cues, live jump, leave loop, WAV markers (`region.*`); markers from other formats open |
| A3 Lyrics, chords, PDF, editor, second screen | partial: lyrics with markup and ChordPro chords (`song.set {lyrics}`), transpose/capo, playing-line highlight, scrolling per song (jump to the line, smooth, constant speed, off: `song.set {lyrics_scroll}`), editor with formatting, time stamps (`lyrics.stamp_line`) and find and replace (`lyrics.replace`), import of text/ChordPro/LRC (`lyrics.import`); PDF and second screen planned |
| A5 MIDI, macros, sync, remote actions | planned M3 |
| A6 Stage UI | partial: sidebar hides during playback, clock modes, audio indicator, status messages; rest planned M3 |
| A7 Band network | planned M8 |
| B1-B8 Lighting | planned M4-M8 |
| C Merge (lighting lane, shared clock) | planned M7 (clock and lane field exist) |
| D Replacements (CLI, control channel, MCP) | done for automation; rest with their features |
| Languages EN and DE | partial: English built in (default), other languages are language files (`crates/actcraft-ui/i18n/<code>.tsv`, German shipped, completeness checked by a test); status messages translated through templates; command error texts still English |
