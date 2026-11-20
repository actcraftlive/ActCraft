# ActCraft

<img src="assets/logo/actcraft-logo.png" alt="ActCraft logo: a drawn octopus" width="160">

Show control for live musicians, in one program and one show file:

- **Backing tracks**: multitrack playback with a mixer, output buses, mute groups, a metronome
  with count-in, setlists, a play queue and history. Lyrics, chords, sheet music and MIDI
  automation follow in the next milestones.
- **Lighting** (planned): DMX over Art-Net, sACN and USB interfaces, fixtures, presets, effects,
  cues and live control pages, driven by the same clock as the music.

ActCraft is written in Rust. The window uses egui on wgpu; audio goes through the operating
system (WASAPI, Core Audio, ALSA/PipeWire). It runs on Windows, macOS and Linux.

## Status

Milestone M0 (workspace, command registry, CLI, control channel, MCP server, build gate) is in
place, and M1 (audio engine, mixer, metronome, library, playlists, queue) is partly done. See
[ROADMAP.md](ROADMAP.md) and [docs/parity.md](docs/parity.md).

## Build and run

```sh
cargo run -p actcraft --bin actcraft            # the window
cargo run -p actcraft --bin actcraft-cli -- commands
cargo xtask ci                                  # the gate: run before every commit
```

## Remote control

Everything the window does is a command with an id. The same commands work from:

- the command line: `actcraft-cli run song.add '{"title":"Opener","bpm":120}' --show gig.actcraft --save`
- the running app over a loopback JSON-RPC channel (port 47380):
  `actcraft-cli send transport.toggle`
- MCP clients: `actcraft-cli mcp --automation-read-root DIR --automation-write-root DIR`
  (file access stays inside those folders).

## Licence

`MIT OR Apache-2.0`, at your option. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE). Third-party notes are in [ATTRIBUTION.md](ATTRIBUTION.md) and
[docs/licences.md](docs/licences.md).
