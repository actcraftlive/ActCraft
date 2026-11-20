//! Layer rules: which ActCraft crates may depend on which.
//!
//! Engine crates (layers 0-3) build for wasm and do no I/O. The platform and control crates do the
//! I/O. The UI is thin: it may use the engine but not the platform, so every feature stays
//! reachable without the window. Only the app crate wires everything together.

use std::path::Path;

use serde_json::Value;

/// Each crate and the ActCraft crates it may depend on (normal and build dependencies).
const ALLOWED: &[(&str, &[&str])] = &[
    ("actcraft-command", &[]),
    ("actcraft-show", &[]),
    ("actcraft-audio", &[]),
    ("actcraft-library", &["actcraft-show"]),
    ("actcraft-engine", &["actcraft-command", "actcraft-show", "actcraft-library", "actcraft-audio"]),
    ("actcraft-platform", &["actcraft-engine", "actcraft-audio"]),
    ("actcraft-control", &["actcraft-engine"]),
    ("actcraft-ui", &["actcraft-engine", "actcraft-command", "actcraft-show"]),
    ("actcraft", &["actcraft-engine", "actcraft-platform", "actcraft-control", "actcraft-ui"]),
];

/// Native-only crates the engine layers must never pull in directly.
const NATIVE_ONLY: &[&str] = &["cpal", "eframe", "egui", "winit", "wgpu", "craft-atomic"];

pub fn check(root: &Path) -> Result<(), String> {
    let out = craft_xtask::runner::cargo(root)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata output: {e}"))?;
    let packages = meta.get("packages").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut problems = Vec::new();
    for p in &packages {
        let name = p.get("name").and_then(Value::as_str).unwrap_or_default();
        if !name.starts_with("actcraft") {
            continue;
        }
        let Some((_, allowed)) = ALLOWED.iter().find(|(n, _)| *n == name) else {
            problems.push(format!("{name} has no layer; add it to xtask/src/layers.rs"));
            continue;
        };
        let engine_layer = crate::WASM_CRATES.contains(&name);
        for d in p.get("dependencies").and_then(Value::as_array).into_iter().flatten() {
            let dep = d.get("name").and_then(Value::as_str).unwrap_or_default();
            let kind = d.get("kind").and_then(Value::as_str);
            if kind == Some("dev") {
                continue;
            }
            if dep.starts_with("actcraft") && !allowed.contains(&dep) {
                problems.push(format!("{name} may not depend on {dep}"));
            }
            if engine_layer && NATIVE_ONLY.contains(&dep) {
                problems.push(format!("{name} is an engine crate and may not depend on {dep} (native only)"));
            }
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join("\n")) }
}
