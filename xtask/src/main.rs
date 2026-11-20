//! `cargo xtask <task>`: ActCraft's build gate.
//!
//! - `ci`: fmt, clippy (warnings are errors), tests, layers, wasm, licences, assets — in that
//!   order, stopping at the first failure. Run it before every commit.
//! - `layers`, `wasm`, `licences`, `assets`: one step on its own.
//!
//! `--skip-missing-targets` lets the wasm step pass with a loud note when the
//! `wasm32-unknown-unknown` target isn't installed on this computer; CI never passes it.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod layers;
mod licences;

use std::path::{Path, PathBuf};
use std::process::Command;

use craft_xtask::assets::{self, Policy};
use craft_xtask::runner::{self, RunOptions, Step, TestMode};

/// Crates whose code must build for `wasm32-unknown-unknown` (everything below the platform layer).
pub const WASM_CRATES: &[&str] =
    &["actcraft-command", "actcraft-show", "actcraft-library", "actcraft-audio", "actcraft-lyrics", "actcraft-engine"];

/// Asset policy: every image, sound, document or font in the repository needs an ATTRIBUTION.md
/// row; font files are not allowed at all (they live in storytold/craft-fonts).
const ASSETS: Policy<'static> = Policy {
    forbidden_ext: &["ttf", "otf", "ttc", "otc", "woff", "woff2"],
    skip_folders: &["target"],
    ..Policy::new("ATTRIBUTION.md")
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).take(8).collect();
    let task = args.first().map_or("ci", String::as_str);
    let skip_missing = args.iter().any(|a| a == "--skip-missing-targets");
    let result = match runner::root(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        Ok(root) if task == "icon" => icon(&root, args.get(1).map(String::as_str)),
        Ok(root) => run(task, &root, skip_missing),
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = result {
        eprintln!("xtask: {e}");
        std::process::exit(1);
    }
}

/// `cargo xtask icon [SOURCE]`: makes the logo files the app and docs use from a large source
/// picture: `assets/logo/actcraft-logo.png` (512 px) and `assets/logo/actcraft-icon-256.png`
/// (the window icon, embedded in the binary).
fn icon(root: &Path, source: Option<&str>) -> Result<(), String> {
    let dir = root.join("assets").join("logo");
    let src = source.map_or_else(|| dir.join("actcraft-logo.png"), PathBuf::from);
    let img = image::open(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    // The drawing is finely detailed, so 512 px keeps the repository light.
    for (size, name) in [(512, "actcraft-logo.png"), (256, "actcraft-icon-256.png")] {
        let out = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3).to_rgba8();
        let path = dir.join(name);
        out.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

fn run(task: &str, root: &Path, skip_missing: bool) -> Result<(), String> {
    let root = root.to_path_buf();
    let mut steps: Vec<Step<'_>> = Vec::new();
    let want = |name: &str| task == "ci" || task == name;
    if task == "ci" {
        let r = root.clone();
        steps.push(Step::new("fmt", move |log| {
            let mut c = runner::cargo(&r);
            c.args(["fmt", "--all", "--", "--check"]);
            runner::run(c, log.command("cargo fmt --all -- --check"))
        }));
        let r = root.clone();
        steps.push(Step::new("clippy", move |log| {
            let mut c = runner::cargo(&r);
            c.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
            runner::run(c, log.command("cargo clippy --workspace --all-targets -- -D warnings"))
        }));
        let r = root.clone();
        steps.push(Step::new("test", move |log| {
            let (c, label) = runner::nextest_or_test(&r, "xtask", TestMode::CargoTest, &["--lib", "--bins", "--tests"])
                .map_err(|e| e.to_string())?;
            runner::run(c, log.command(&label))
        }));
    }
    if want("layers") {
        let r = root.clone();
        steps.push(Step::new("layers", move |log| {
            log.command("cargo metadata (layer rules)");
            layers::check(&r)
        }));
    }
    if want("wasm") {
        let r = root.clone();
        steps.push(Step::new("wasm", move |log| {
            if !target_installed(&r, "wasm32-unknown-unknown") {
                let msg =
                    "the wasm32-unknown-unknown target is not installed (rustup target add wasm32-unknown-unknown)";
                if skip_missing {
                    eprintln!("NOTE: wasm step NOT RUN: {msg}");
                    return Ok(());
                }
                return Err(msg.to_owned());
            }
            let mut c = runner::cargo(&r);
            c.args(["check", "--target", "wasm32-unknown-unknown"]);
            for k in WASM_CRATES {
                c.args(["-p", k]);
            }
            runner::run(c, log.command("cargo check --target wasm32-unknown-unknown (engine crates)"))
        }));
    }
    if want("licences") {
        let r = root.clone();
        steps.push(Step::new("licences", move |log| {
            log.command("cargo metadata (licence allowlist)");
            licences::check(&r)
        }));
    }
    if want("assets") {
        let r = root.clone();
        steps.push(Step::new("assets", move |log| {
            log.command("assets attribution check");
            let report = assets::run(&r, &ASSETS).map_err(|e| e.to_string())?;
            if report.is_ok() {
                Ok(())
            } else {
                Err(report.violations.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))
            }
        }));
    }
    if steps.is_empty() {
        return Err(format!("unknown task `{task}`; use ci, layers, wasm, licences or assets"));
    }
    runner::run_steps(&mut steps, &RunOptions::default().with_title(format!("cargo xtask {task}")))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn target_installed(root: &PathBuf, target: &str) -> bool {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let out = Command::new(rustc).current_dir(root).args(["--print", "target-libdir", "--target", target]).output();
    match out {
        Ok(o) if o.status.success() => {
            let dir = String::from_utf8_lossy(&o.stdout).trim().to_owned();
            std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_some())
        }
        _ => false,
    }
}
