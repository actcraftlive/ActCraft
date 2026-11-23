//! Licence check: every dependency resolved for any platform ActCraft ships on must carry an SPDX
//! licence expression made only of allowlisted identifiers (house rules, licence allowlist: "an
//! expression is allowed when every identifier is in this list").
//!
//! So every side of `OR` and `AND` must be allowed, and `X WITH Y` must match an allowlisted
//! exception entry. The old `MIT/Apache-2.0` form is read as `OR`. A dependency with no `license`
//! field fails: someone has to read its licence file and decide.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

/// The allowlist from the house rules.
const ALLOWED: &[&str] = &[
    "ISC",
    "MIT",
    "0BSD",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "Zlib",
    "BSL-1.0",
    "Unicode-3.0",
    "Unlicense",
    "IJG",
    "NCSA",
    "CDLA-Permissive-2.0",
    "OFL-1.1",
    "Ubuntu-font-1.0",
    "Bitstream-Vera",
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "LicenseRef-Public-Domain",
    // Allowed only for third-party dependencies used unmodified; nothing is vendored here.
    "MPL-2.0",
];

/// Dual-licensed dependencies we take under one allowlisted option, with the expression they
/// declare. Same practice as the other Craft apps (craft-anything, anicraft); each row is also in
/// docs/licences.md. A changed expression fails the check again, so it gets a fresh look.
const ELECTED: &[(&str, &str, &str)] = &[
    // egui's epaint needs it; we use it under Apache-2.0, never GPL-2.0.
    ("self_cell", "Apache-2.0 OR GPL-2.0-only", "Apache-2.0"),
    // The zip crate (through craft-zip, for show packages) needs it; used under Apache-2.0.
    ("constant_time_eq", "CC0-1.0 OR MIT-0 OR Apache-2.0", "Apache-2.0"),
    // craft-update (checking for a newer release on request) needs it; used under Apache-2.0.
    ("dunce", "CC0-1.0 OR MIT-0 OR Apache-2.0", "Apache-2.0"),
];

/// Platforms we ship on (and the wasm build of the engine crates).
const PLATFORMS: &[&str] = &[
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "wasm32-unknown-unknown",
];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Id(String),
    And,
    Or,
    With,
    Open,
    Close,
}

fn tokenize(expr: &str) -> Vec<Tok> {
    let spaced = expr.replace('(', " ( ").replace(')', " ) ").replace('/', " OR ");
    spaced
        .split_whitespace()
        .map(|w| match w {
            "AND" => Tok::And,
            "OR" => Tok::Or,
            "WITH" => Tok::With,
            "(" => Tok::Open,
            ")" => Tok::Close,
            other => Tok::Id(other.trim_end_matches('+').to_owned()),
        })
        .collect()
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn or(&mut self, depth: usize) -> Option<bool> {
        let mut ok = self.and(depth)?;
        while self.toks.get(self.pos) == Some(&Tok::Or) {
            self.pos += 1;
            let rhs = self.and(depth)?;
            // Strict: every identifier must be allowed, even on the side we would not pick.
            ok = ok && rhs;
        }
        Some(ok)
    }

    fn and(&mut self, depth: usize) -> Option<bool> {
        let mut ok = self.atom(depth)?;
        while self.toks.get(self.pos) == Some(&Tok::And) {
            self.pos += 1;
            let rhs = self.atom(depth)?;
            ok = ok && rhs;
        }
        Some(ok)
    }

    fn atom(&mut self, depth: usize) -> Option<bool> {
        if depth > 16 {
            return None;
        }
        match self.toks.get(self.pos).cloned()? {
            Tok::Open => {
                self.pos += 1;
                let v = self.or(depth + 1)?;
                if self.toks.get(self.pos) != Some(&Tok::Close) {
                    return None;
                }
                self.pos += 1;
                Some(v)
            }
            Tok::Id(id) => {
                self.pos += 1;
                if self.toks.get(self.pos) == Some(&Tok::With) {
                    self.pos += 1;
                    let Some(Tok::Id(exc)) = self.toks.get(self.pos).cloned() else { return None };
                    self.pos += 1;
                    return Some(ALLOWED.contains(&format!("{id} WITH {exc}").as_str()));
                }
                Some(ALLOWED.contains(&id.as_str()))
            }
            _ => None,
        }
    }
}

/// Whether an SPDX expression is allowed. A malformed expression is not.
pub fn allowed(expr: &str) -> bool {
    let mut p = Parser { toks: tokenize(expr), pos: 0 };
    matches!(p.or(0), Some(true)) && p.pos == p.toks.len()
}

pub fn check(root: &Path) -> Result<(), String> {
    // name@version -> licence
    let mut seen: BTreeMap<String, Option<String>> = BTreeMap::new();
    for platform in PLATFORMS {
        let out = craft_xtask::runner::cargo(root)
            .args(["metadata", "--format-version", "1", "--filter-platform", platform])
            .output()
            .map_err(|e| format!("cargo metadata: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "cargo metadata --filter-platform {platform} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata output: {e}"))?;
        let resolved: Vec<&str> = meta
            .pointer("/resolve/nodes")
            .and_then(Value::as_array)
            .map(|nodes| nodes.iter().filter_map(|n| n.get("id").and_then(Value::as_str)).collect())
            .unwrap_or_default();
        for p in meta.get("packages").and_then(Value::as_array).into_iter().flatten() {
            let id = p.get("id").and_then(Value::as_str).unwrap_or_default();
            if !resolved.contains(&id) {
                continue;
            }
            let name = p.get("name").and_then(Value::as_str).unwrap_or_default();
            let version = p.get("version").and_then(Value::as_str).unwrap_or_default();
            let licence = p.get("license").and_then(Value::as_str).map(str::to_owned);
            seen.insert(format!("{name}@{version}"), licence);
        }
    }
    let elected = |pkg: &str, licence: &str| {
        let name = pkg.split('@').next().unwrap_or_default();
        ELECTED.iter().any(|(n, expr, choice)| *n == name && *expr == licence && allowed(choice))
    };
    let bad: Vec<String> = seen
        .iter()
        .filter(|(p, l)| !l.as_deref().is_some_and(|l| allowed(l) || elected(p, l)))
        .map(|(p, l)| format!("{p}: {}", l.as_deref().unwrap_or("no licence field")))
        .collect();
    if bad.is_empty() {
        println!("{} dependencies checked, all licences allowed", seen.len());
        Ok(())
    } else {
        Err(format!("licences outside the allowlist:\n{}", bad.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::allowed;

    #[test]
    fn spdx_expressions() {
        assert!(allowed("MIT OR Apache-2.0"));
        assert!(allowed("(MIT OR Apache-2.0) AND Unicode-3.0"));
        assert!(allowed("MIT/Apache-2.0"));
        assert!(allowed("Apache-2.0 WITH LLVM-exception"));
        assert!(!allowed("MIT OR Apache-2.0 OR LGPL-2.1-or-later"));
        assert!(!allowed("GPL-3.0"));
        assert!(!allowed("MIT AND GPL-2.0"));
        assert!(!allowed("GPL-2.0 WITH Classpath-exception-2.0"));
        assert!(!allowed("(MIT"));
        assert!(!allowed(""));
    }
}
