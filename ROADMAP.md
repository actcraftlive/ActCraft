# Roadmap

Each milestone ends green on `cargo xtask ci` with `docs/parity.md` updated.

| Milestone | Scope | Status |
|---|---|---|
| M0 | Workspace, layers, command registry, CLI, control channel, MCP, CI gate, AGENTS.md | done (CI workflow file still to add through craft-common-actions) |
| M1 | Audio engine, mixer, buses, metronome, library, playlists, queue | in progress |
| M2 | Regions, lyrics/chords/PDF, editor, scrolling, second screen | planned |
| M3 | MIDI in/out/sync, macros, remote actions, assignable controls | planned |
| M4 | DMX output (Art-Net, sACN, USB), patch, fixture editor, fixture library import, DMX monitor | planned |
| M5 | 2D preview, programming, presets, sequences, effects engine | planned |
| M6 | Cues, cue lists, control pages, blind/freeze, beat sync | planned |
| M7 | Lighting lane, shared clock, one show file | planned |
| M8 | Band network, redundancy, OSC, DMX-In, joystick, Hue | planned |
| M9 | Packaging: MSI and zip, DMG, AppImage/DEB/RPM/tarball | planned |

## Open items in M1

- Background loading of audio files (decoding now runs on the command thread).
- Native open/save dialogs (craft-dialogs); paths are typed into a dialog for now.
- Export, backup and restore of songs and playlists.
- Track colour picker and EQ panel in the mixer; drag-and-drop reorder in sets.
- Effect plugin hosting: choose a plugin format whose SDK licence passes the allowlist.
- Translation catalogs (English and German) through craft-i18n.
- Keyboard-driven accessibility tests (egui kittest) for the window.
- Screen reader announcements: the status message is exposed as a label; a proper live region
  is still to do.
