# Attribution

Every asset in this repository (images, icons, sounds, documents) has a row here, added in the
same change as the asset. Icons are drawn in code where possible. Font files are not kept in this
repository; the window uses the fonts built into egui (`epaint_default_fonts`, a dependency under
OFL-1.1, Ubuntu-font-1.0 and MIT/Apache-2.0).

| Path | Author / source | Licence | Notes |
|---|---|---|---|
| `assets/logo/actcraft-logo.png` | ActCraft project owner; generated with Google Gemini from the owner's own prompt (2026) | MIT OR Apache-2.0 | ActCraft logo (octopus drawing), scaled to 512 px by `cargo xtask icon` |
| `assets/logo/actcraft-icon-256.png` | Same source as the logo | MIT OR Apache-2.0 | Window icon, 256 px, embedded in the `actcraft` binary |
| `assets/logo/actcraft.ico` | Same source as the logo | MIT OR Apache-2.0 | Windows program icon (16 to 256 px), made by `cargo xtask icon`, embedded in `actcraft.exe` |

## Dependencies taken under one option of a dual licence

| Crate | Declared | Used under | Why |
|---|---|---|---|
| `self_cell` | Apache-2.0 OR GPL-2.0-only | Apache-2.0 | Required by egui's `epaint`; same choice as the other Craft apps |
