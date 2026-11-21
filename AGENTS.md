<!-- craft-house-rules:start core source=5b1a2b1982fe7c774739d06d4a3818033cf4884c sha256=bee0ffafad196b68c76425770e344e588463bbda685ab41581fcbabb85dfc930 -->
## House rules

These apply in every Craft repository. A later rule may be stricter, not weaker. The source is `standards/house-rules-core.md` in `storytold/craft-contrib`. Propose changes there, not in a copy.

1. **A person is accountable.** Whoever runs an agent answers for what it opens and for its review comments.
2. **Check, then claim.** Before writing code, read the issue comments, search pull requests for `#N` and the keywords, and check recent commits on `main`. Comment on the issue first. Release a claim you will not finish.
3. **Verify, don't guess.** Open a definition before using it. Confirm the cause in the code, even when someone names a location.
4. **Keep fixes narrow and tested.** Fix what the issue describes, where the bug is. Add a test that fails without the fix.
5. **Review the diff before opening a pull request.** Identifiers exist as used, the test fails without the fix, nothing unrelated changed, and the description is true.
6. **Look for a better pull request.** If another fixes the same thing, close yours when it is better or equal, and say why.
7. **Report only what ran.** Say what you tested and what you could not run. Without a local build or test, say so and rely on CI before marking ready.
8. **`main` changes through a pull request.** Do not push to it. Branch protection enforces this. There is no exception.
9. **Stop and ask only about** licensing outside the allowlist, publishing, releases, a new remote, secrets, spending money, destructive git, and anything else hard to undo. Otherwise keep working. `plan/` is a gitignored notebook, not rules. Without it, do not ask for it and do not stop.
10. **Keep secrets and personal details out** of the repository, logs and requests.
11. **CI stays shared.** Workflows call `storytold/craft-common-actions`. Do not change CI in a feature or fix pull request. If CI cannot cover a change, open an issue there.
12. **Guards say why.** A check that looks removable says what it prevents. Read that before changing it.
13. **Instructions must be readable by everyone.** No notes about one machine, and no links to files a reader cannot open. `craftrules` is retired. Do not link to it.
14. **Keep pull requests moving.** Concurrent pull requests are fine; unanswered actionable feedback is not. Respond in 7 days, a reminder at 14, closure eligible at 30, from when an actionable request waits on the author. Suggestions and unconfirmed bot findings do not start the clock. A fix, explanation or agreed plan resets it; unrelated activity does not. Waiting on a reviewer or maintainer pauses it and does not count against the author. A maintainer decides closure; it is never automatic. Reuse search results, do not poll, and stop at a rate limit.
15. **No agent attribution.** A pull request does not say an agent wrote it. A repository rule may not require that.
16. **Shared libraries first for Craft apps.** Before adding or changing reusable file-format, codec, protocol or UI code, check `storytold/craft-libs` and relevant open pull requests. Reuse its public APIs or extend the appropriate shared library instead of duplicating an implementation in an app. Keep app-specific policy and model mapping local; preserve behavior, security and resource limits, attribution, layering and regression coverage. Document API gaps and maintainer-approved exceptions rather than silently dropping behavior. This does not authorize dependencies on sibling application crates or override existing platform, licensing or dependency-source requirements.
<!-- craft-house-rules:end -->

<!-- craft-house-rules:start rust source=5b1a2b1982fe7c774739d06d4a3818033cf4884c sha256=56ef2b873de68940cf903069bbd0927b119f5c9ec2717f7f627041d2ebe8e4f2 -->
## Rust Craft repositories

These apply to native Craft apps and to shared Rust libraries. Select them by repository type. A repository that merely contains some Rust does not take this block. Rules later in a repository's file may be stricter, but not weaker. The source is `standards/house-rules-rust.md` in [storytold/craft-contrib](https://github.com/storytold/craft-contrib).

1. **Never crash.** Non-test code does not panic and has no `unsafe`, except where the repository file names an exception. Return errors, treat input as hostile, and bound recursion and allocations. New production crate roots copy the `deny` attribute from an existing crate: `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::unimplemented`, `clippy::todo`, `clippy::unreachable`. Every crash fix comes with a test that panicked before the fix.
2. **Clean room.** Learn from observed behaviour and public specifications. Do not copy another product's code, assets, strings or artwork. Check a dependency's licence before adding it.
3. **Assets and fonts.** Every asset is original or suitably licensed, and is recorded in `ATTRIBUTION.md` in the same change. Font files live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), not in the app repository.
4. **Rust only for native Craft apps.** No handwritten JavaScript or TypeScript, and no Tauri, Electron or webview shell, in those apps. Shared tooling follows its documented technical scope and maintainer-approved exceptions. Craft PR Review's existing Tauri and TypeScript UI is one such exception. It does not relax this rule for native apps. Preserve any existing web build.
5. **Time goes through `chrono`.** Do not hand-roll calendar conversion. For dates beyond what `chrono` can represent, follow [standards/time.md](https://github.com/storytold/craft-contrib/blob/main/standards/time.md).
6. **Contributor credits.** Add or edit only your own person's entry in `contributors/people.toml`, and only after asking them. Never guess a name or hand-edit `contributors.json`.

## Licence allowlist

Dependencies and assets use SPDX identifiers. An expression is allowed when every identifier is in this list. A repository may name a shorter list. It may not add an identifier here by editing its own copy of these rules.

- `ISC`
- `MIT`
- `BSD-2-Clause`
- `BSD-3-Clause`
- `Apache-2.0`
- `Unicode-3.0`
- `OFL-1.1`
- `Ubuntu-font-1.0`
- `Bitstream-Vera`
- `CC0-1.0`
- `CC-BY-4.0`
- `CC-BY-SA-4.0`
- `Zlib`
- `BSL-1.0`
- `0BSD`
- `Unlicense`
- `IJG`
- `NCSA`
- `CDLA-Permissive-2.0`
- `Apache-2.0 WITH LLVM-exception`
- `LicenseRef-Public-Domain`

`MPL-2.0` is allowed for third-party dependencies used unmodified; vendoring, patching or copying its code needs a maintainer's approval. `MPL-2.0+` counts as `MPL-2.0`.

`LicenseRef-Storyteller-Trademark` is not in this list. It covers Storyteller brand marks only, where a repository already says so.
<!-- craft-house-rules:end -->

## Accessibility

These apply to native Craft apps and to shared libraries that draw interface. The source is `standards/house-rules-accessibility.md` in `storytold/craft-contrib`. The full rules are [standards/accessibility.md](https://github.com/storytold/craft-contrib/blob/main/standards/accessibility.md).

People use the apps with screen readers, braille displays, magnifiers, high contrast, large text, voice control or only a keyboard. They reach the interface only through its AccessKit tree, keyboard focus and colours. A UI change is not done until it works for them. The bar is WCAG 2.2 AA as applied to software.

1. **Design first.** Name the pattern (menu, tab list, toolbar, tree, grid, list, dialog, slider, canvas) and follow its keyboard model and roles from the WAI-ARIA Authoring Practices Guide and the platform's guidelines. Observe how the reference app exposes it with Accessibility Insights, Accessibility Inspector or Accerciser.
2. **Keyboard.** Every action works from the keyboard, in visual order, with no trap. Esc closes and returns focus. Alt or F10 and F6 behave as on the platform. Move egui's focus (`request_focus`), never only a highlight.
3. **Semantics.** Every control has a role, a translated name that starts with its visible label, and its state and value. Shortcuts and access keys are set on the node, in the current platform's form. A drawn surface publishes nodes for its content, with caret, selection and correct editability.
4. **Feedback.** Announce changes away from focus through a live region.
5. **Seeing.** Contrast at least 4.5:1 for text and 3:1 for controls and focus, in both themes. Never colour alone. Follow the system's high contrast, text size and reduced motion settings.
6. **Pointer.** Anything done by dragging can be done without dragging.
7. **Check the tree, not only the screenshot.** Each new or changed control, dialog or keyboard path gets a kittest test that drives it by keyboard and asserts roles, names, states, focus and announcements. If the helpers do not exist yet, say so in the pull request. Keep `with_accessibility_check` on and CI's platform checks green. With `ui.screenshot`, also read the accessibility tree.
8. **Rule out known failures:** focus left in the document behind a menu, ribbon or KeyTips; editable surfaces reported read-only; dialogs that do not take or return focus, or leave controls behind them reachable; painted rows with no node; identical names; another platform's shortcut text in a name; silent state changes; drag-only actions.
9. **Be honest in the pull request.** Its Accessibility section says what the tests and the tree cover. Tree tests never prove "works with screen readers". Ask for `needs screen reader test` only for an accessibility fix or accessibility logic of its own, and never hold a pull request back for that test.

## Naming

Follow [standards/naming.md](https://github.com/storytold/craft-contrib/blob/main/standards/naming.md). Feature names are our own words; accurate compatibility names for open protocols and formats stay (MIDI, MTC, MIDI Clock, OSC, Art-Net, sACN/E1.31, OS2L, ChordPro, LRC, PDF, WAV, FLAC, MP3, Philips Hue). Checked names go in `docs/feature-names.md`.

## This repository

ActCraft is a native show-control app for live musicians: multitrack backing tracks with lyrics, setlists and MIDI, and DMX lighting, on one clock and in one show file. Rust only; the window is egui/eframe on wgpu.

### Layout

| Crate | Layer | What it is |
|---|---|---|
| `actcraft-command` | 0 | The command registry: id, label, menu path, shortcut, params doc, enabled, run; dispatch inside a last-resort guard |
| `actcraft-show` | 1 | Show model and the versioned `.actcraft` format (older files load, unknown fields kept) |
| `actcraft-audio` | 1 | Realtime renderer (no allocation or locks in the callback), EQ, buses, mute groups, metronome, crossfades, decoding, peaks |
| `actcraft-library` | 2 | Search, smart playlists, queue, history, filename tags, duplicates, reordering |
| `actcraft-engine` | 3 | Engine state and every command; I/O only through the `Services` trait |
| `actcraft-platform` | 4 | Native services: files (atomic writes), folder-restricted access, OS audio output |
| `actcraft-control` | 4 | JSON-RPC control channel on loopback, MCP server on stdio |
| `actcraft-ui` | 5 | Thin egui window: reads engine state, changes it only through commands; colours and sizes from theme tokens |
| `actcraft` | 6 | Binaries `actcraft` (window) and `actcraft-cli` |
| `xtask` | - | The gate |

Layers 0-3 build for `wasm32-unknown-unknown` and may not depend on native crates. `cargo xtask layers` enforces the table.

### Commands

- Gate before every commit: `cargo xtask ci` (fmt, clippy `-D warnings`, tests, layers, wasm, licences, assets).
- CI runs the same gate on Linux and Windows through `storytold/craft-common-actions` (`.github/workflows/ci.yml`), and checks the house-rule blocks above (`house-rules.yml`). `main` is protected: changes arrive through pull requests whose checks pass.
- One step: `cargo xtask layers|wasm|licences|assets`.
- Run: `cargo run -p actcraft --bin actcraft` (window), `cargo run -p actcraft --bin actcraft-cli -- commands`.
- Agent entry point: `actcraft-cli mcp --automation-read-root DIR --automation-write-root DIR` refuses to start without both roots and refuses files outside them.

### Conventions

- Every action is a command in `crates/actcraft-engine/src/commands.rs`; the window, CLI, control channel and MCP dispatch by id. A new feature adds its command first.
- Every command survives the hostile-params sweep (`every_command_survives_hostile_params`); parsers have a fuzz sweep in `tests/it/main.rs` and a cargo-fuzz target in `fuzz/`.
- Visible text goes through `tr()` with English as the key. English is built in and the default; every other language is a language file `crates/actcraft-ui/i18n/<code>.tsv` (craft-i18n catalog format), picked up by the build. New window text needs a row in every language file (`docs/translating.md`).
- `unsafe` is forbidden everywhere; there is no exception module.
- Dual-licensed dependencies taken under one allowlisted option are listed in `xtask/src/licences.rs` and `docs/licences.md`.
- `plan/` is a gitignored notebook for evidence about the reference apps; reference product names never appear in UI, docs prose, code identifiers or release text.
- Parity with the requested feature list is tracked in `docs/parity.md` and `ROADMAP.md`.
