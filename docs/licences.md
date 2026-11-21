# Licences

ActCraft is licensed `MIT OR Apache-2.0`.

`cargo xtask licences` reads `cargo metadata` for every platform ActCraft ships on (Windows x64 and
arm64, Linux x86_64 and aarch64, macOS Intel and Apple silicon, and wasm32 for the engine crates)
and fails when a dependency's licence expression uses an identifier outside the Craft allowlist
([standards/licences.md](https://github.com/storytold/craft-contrib/blob/main/standards/licences.md)).
Every identifier must be allowed, on both sides of `OR` too.

## Elections

Dual-licensed dependencies we take under one allowlisted option. A changed expression fails the
check again so it gets a fresh look.

| Crate | Declared | Used under | Reason |
|---|---|---|---|
| `self_cell` | Apache-2.0 OR GPL-2.0-only | Apache-2.0 | egui's `epaint` requires it; the other Craft apps (craft-anything, anicraft) record the same choice |
| `constant_time_eq` | CC0-1.0 OR MIT-0 OR Apache-2.0 | Apache-2.0 | the `zip` crate requires it (through craft-zip, for show packages); MIT-0 is not on the allowlist, Apache-2.0 is |

## Out of scope

Ableton Link (GPL-2.0), OLA (GPL-2.0/LGPL-2.1) and the ASIO SDK are not used. Plugin formats are
added only when their SDK licence passes the allowlist.
