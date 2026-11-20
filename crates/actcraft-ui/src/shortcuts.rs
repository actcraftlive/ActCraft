//! Keyboard shortcuts and menus built from the command registry.

use actcraft_engine::registry;
use egui::{Key, KeyboardShortcut, Modifiers};

/// Parses a platform-neutral shortcut like `Ctrl+S`, `Space` or `M`. `Ctrl` means Cmd on macOS.
pub fn parse(s: &str) -> Option<KeyboardShortcut> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    for part in s.split('+') {
        match part {
            "Ctrl" => mods |= Modifiers::COMMAND,
            "Shift" => mods |= Modifiers::SHIFT,
            "Alt" => mods |= Modifiers::ALT,
            other => key = Key::from_name(other),
        }
    }
    key.map(|k| KeyboardShortcut::new(mods, k))
}

/// How a shortcut is shown in menus on this platform.
pub fn display(s: &str) -> String {
    if cfg!(target_os = "macos") { s.replace("Ctrl+", "⌘") } else { s.to_owned() }
}

/// Every command with a shortcut, most modifiers first so `Ctrl+Right` wins over `Right`.
pub fn table() -> Vec<(KeyboardShortcut, &'static str)> {
    let mut out: Vec<(KeyboardShortcut, &'static str)> =
        registry().iter().filter_map(|c| c.spec.shortcut.and_then(parse).map(|k| (k, c.spec.id))).collect();
    out.sort_by_key(|(k, _)| {
        std::cmp::Reverse(u8::from(k.modifiers.command) + u8::from(k.modifiers.shift) + u8::from(k.modifiers.alt))
    });
    out
}

/// Top-level menus in the order their first command appears.
pub fn menu_roots() -> Vec<&'static str> {
    let mut roots: Vec<&'static str> = Vec::new();
    for c in registry().iter() {
        if let Some(root) = c.spec.menu.split('/').next().filter(|r| !r.is_empty())
            && !roots.contains(&root)
        {
            roots.push(root);
        }
    }
    // Help goes last, as on every platform.
    if let Some(i) = roots.iter().position(|r| *r == "Help") {
        let help = roots.remove(i);
        roots.push(help);
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_shortcut_parses() {
        for c in registry().iter() {
            if let Some(s) = c.spec.shortcut {
                assert!(parse(s).is_some(), "{} has unknown shortcut {s}", c.spec.id);
            }
        }
    }
}
